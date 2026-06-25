use rusqlite::{Connection, params};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use md5;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Equipo {
    pub codigo_rfid: String,
    pub item: Option<String>,
    pub numero_serie: Option<String>,
    pub descripcion: Option<String>,
    pub marca: Option<String>,
    pub modelo: Option<String>,
    pub categoria: Option<String>,
    pub cantidad: Option<i32>,
    pub almacen: Option<String>,
    pub permiso_salida: Option<bool>,
    pub estado_ubicacion: Option<String>,
    pub status: String,
}

// ─── ESTADOS GLOBALES ─────────────────────────────────────────────────────────
pub struct DbState(pub Mutex<Connection>);
pub struct RfidState(pub Mutex<bool>);
pub struct RfidTxState(pub Mutex<Option<mpsc::Sender<Vec<u8>>>>);

// ─── HELPER MD5 ──────────────────────────────────────────────────────────────
fn hash_md5(input: &str) -> String {
    format!("{:x}", md5::compute(input))
}

// ─── CONSTRUCTOR DE COMANDOS RFID ─────────────────────────────────────────────
fn build_command(command: u8, data: &[u8]) -> Vec<u8> {
    let length = (8 + data.len()) as u16;
    let mut checksum: u8 = ((length >> 8) as u8) ^ (length as u8) ^ command;
    for b in data {
        checksum ^= b;
    }
    let mut frame = Vec::new();
    frame.push(0xA5);
    frame.push(0x5A);
    frame.push((length >> 8) as u8);
    frame.push(length as u8);
    frame.push(command);
    frame.extend_from_slice(data);
    frame.push(checksum);
    frame.push(0x0D);
    frame.push(0x0A);
    frame
}

// ─── INICIALIZAR BASE DE DATOS ────────────────────────────────────────────────
fn init_db(conn: &Connection) {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = 10000;
         PRAGMA temp_store = MEMORY;
         PRAGMA mmap_size = 268435456;
         
         CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY,
            username      TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at    TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS equipos_glef (
            codigo_rfid   TEXT PRIMARY KEY,
            item          TEXT,
            numero_serie  TEXT,
            descripcion   TEXT,
            marca         TEXT,
            modelo        TEXT,
            categoria     TEXT,
            cantidad      INTEGER,
            almacen       TEXT,
            permiso_salida INTEGER DEFAULT 0,
            estado_ubicacion TEXT DEFAULT 'En Oficina',
            updated_at    TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS LecturasRFID (
            Id           INTEGER PRIMARY KEY AUTOINCREMENT,
            EPC          TEXT NOT NULL,
            FechaLectura TEXT NOT NULL DEFAULT (datetime('now')),
            sincronizado INTEGER NOT NULL DEFAULT 0
         );
         
         CREATE INDEX IF NOT EXISTS idx_epc ON LecturasRFID(EPC);
         CREATE INDEX IF NOT EXISTS idx_fecha ON LecturasRFID(FechaLectura);
         CREATE INDEX IF NOT EXISTS idx_sincronizado ON LecturasRFID(sincronizado);"
    )
    .expect("Error creando tablas");

    // Intentamos agregar columnas si la tabla ya existía de antes
    let _ = conn.execute("ALTER TABLE equipos_glef ADD COLUMN almacen TEXT", []);
    let _ = conn.execute("ALTER TABLE equipos_glef ADD COLUMN permiso_salida INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE equipos_glef ADD COLUMN estado_ubicacion TEXT DEFAULT 'En Oficina'", []);

    // Crear usuario admin por defecto solo si no existe
    // Contraseña: 1234 hasheada con MD5
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE username = 'admin'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if count == 0 {
        let password_hash = hash_md5("1234");
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at)
             VALUES (?1, ?2, datetime('now'))",
            params!["admin", password_hash],
        )
        .expect("Error insertando usuario admin");
        println!("✅ Usuario admin creado");
    }
}

// ─── FETCH USUARIOS DEL SERVIDOR ─────────────────────────────────────────────
async fn fetch_users_from_server() -> Option<Vec<(String, String)>> {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(e) => {
            println!("⚠️ Sin conexión al servidor: {}", e);
            return None;
        }
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(e) => {
            println!("⚠️ Error autenticando: {}", e);
            return None;
        }
    };

    let stream = match client
        .query("SELECT username, password_hash FROM users", &[])
        .await
    {
        Ok(s) => s,
        Err(e) => {
            println!("⚠️ Error consultando: {}", e);
            return None;
        }
    };

    let rows = match stream.into_first_result().await {
        Ok(r) => r,
        Err(e) => {
            println!("⚠️ Error leyendo filas: {}", e);
            return None;
        }
    };

    let users = rows
        .iter()
        .filter_map(|row| {
            let username: &str = row.get(0)?;
            let password_hash: &str = row.get(1)?;
            Some((username.to_string(), password_hash.to_string()))
        })
        .collect();

    Some(users)
}

// ─── GUARDAR USUARIOS EN SQLITE ───────────────────────────────────────────────
fn save_users_to_sqlite(conn: &Connection, users: Vec<(String, String)>) -> usize {
    let mut count = 0;
    for (username, password_hash) in &users {
        if conn
            .execute(
                "INSERT INTO users (username, password_hash, created_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(username) DO UPDATE SET
                     password_hash = excluded.password_hash",
                params![username, password_hash],
            )
            .is_ok()
        {
            count += 1;
        }
    }
    count
}

// ─── SINCRONIZACIÓN DE INVENTARIO ─────────────────────────────────────────────
async fn fetch_inventory_from_server() -> Option<Vec<Equipo>> {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(_) => return None,
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(_) => return None,
    };

    let result = client
        .query("SELECT CAST(CODIGO_RFID AS NVARCHAR(MAX)), CAST(ITEM AS NVARCHAR(MAX)), CAST([NUMERO DE SERIE] AS NVARCHAR(MAX)), CAST(DESCRIPCION AS NVARCHAR(MAX)), CAST(MARCA AS NVARCHAR(MAX)), CAST(MODELO AS NVARCHAR(MAX)), CAST(CATEGORIA AS NVARCHAR(MAX)), CAST(CANTIDAD AS INT), CAST([ALMACÉN] AS NVARCHAR(MAX)) FROM EQUIPOS_GLEF", &[])
        .await;

    let stream = match result {
        Ok(s) => s,
        Err(e) => {
            println!("Error en query: {}", e);
            return None;
        }
    };

    let rows = match stream.into_first_result().await {
        Ok(r) => r,
        Err(_) => return None,
    };

    let mut inventory = Vec::new();
    for row in rows {
        let codigo_rfid: Option<&str> = row.get(0);
        let item: Option<&str> = row.get(1);
        let numero_serie: Option<&str> = row.get(2);
        let descripcion: Option<&str> = row.get(3);
        let marca: Option<&str> = row.get(4);
        let modelo: Option<&str> = row.get(5);
        let categoria: Option<&str> = row.get(6);
        let cantidad: Option<i32> = row.get(7);
        let almacen: Option<&str> = row.get(8);

        if let Some(rfid) = codigo_rfid {
            inventory.push(Equipo {
                codigo_rfid: rfid.to_string(),
                item: item.map(|s| s.to_string()),
                numero_serie: numero_serie.map(|s| s.to_string()),
                descripcion: descripcion.map(|s| s.to_string()),
                marca: marca.map(|s| s.to_string()),
                modelo: modelo.map(|s| s.to_string()),
                categoria: categoria.map(|s| s.to_string()),
                cantidad,
                almacen: almacen.map(|s| s.to_string()),
                permiso_salida: None,
                estado_ubicacion: None,
                status: "missing".to_string(),
            });
        }
    }

    Some(inventory)
}

fn save_inventory_to_sqlite(conn: &Connection, inventory: Vec<Equipo>) -> usize {
    let mut count = 0;
    for equipo in &inventory {
        if conn.execute(
            "INSERT INTO equipos_glef (codigo_rfid, item, numero_serie, descripcion, marca, modelo, categoria, cantidad, almacen, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
             ON CONFLICT(codigo_rfid) DO UPDATE SET
                 item = excluded.item,
                 numero_serie = excluded.numero_serie,
                 descripcion = excluded.descripcion,
                 marca = excluded.marca,
                 modelo = excluded.modelo,
                 categoria = excluded.categoria,
                 cantidad = excluded.cantidad,
                 almacen = excluded.almacen,
                 updated_at = excluded.updated_at",
            params![
                equipo.codigo_rfid,
                equipo.item,
                equipo.numero_serie,
                equipo.descripcion,
                equipo.marca,
                equipo.modelo,
                equipo.categoria,
                equipo.cantidad,
                equipo.almacen
            ],
        ).is_ok() {
            count += 1;
        }
    }
    count
}

// ─── GUARDAR BATCH DE EPCs EN SQLITE ──────────────────────────────────────────
fn guardar_epcs_batch(conn: &mut Connection, epcs: &[String]) {
    if epcs.is_empty() {
        return;
    }
    
    let tx = conn.transaction().expect("Error iniciando transacción");
    for epc in epcs {
        match tx.execute(
            "INSERT INTO LecturasRFID (EPC, FechaLectura, sincronizado)
             VALUES (?1, datetime('now'), 0)",
            params![epc],
        ) {
            Ok(_) => println!("💾 SQLite INSERT OK: {}", epc),
            Err(e) => println!("❌ SQLite ERROR: {}", e),
        }
    }
    tx.commit().expect("Error commit transacción");
    println!("💾 Batch SQLite: {} registros", epcs.len());
}

// ─── ACTIVAR ALARMA FÍSICA (RELAY) ────────────────────────────────────────────
async fn activar_alarma_fisica(tx_opt: Option<mpsc::Sender<Vec<u8>>>) {
    println!("🚨 Activando alarma física...");
    
    if let Some(tx) = tx_opt {
        // Relay ON (pines 3-4)
        let frame_on = build_command(0xA1, &[0x09, 0x00, 0x00, 0x01]);
        let _ = tx.send(frame_on).await;
        
        // Esperar 5 segundos
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        // Relay OFF
        let frame_off = build_command(0xA1, &[0x09, 0x00, 0x00, 0x00]);
        let _ = tx.send(frame_off).await;
        println!("✅ Alarma física desactivada");
    } else {
        println!("⚠️ No se pudo activar la alarma física porque el lector no está conectado en Iniciar Lectura.");
    }
}

// ─── COMANDO SALUDAR ──────────────────────────────────────────────────────────
#[tauri::command]
fn saludar(nombre: &str) -> String {
    format!("Hola {}", nombre)
}

// ─── COMANDO LOGIN ────────────────────────────────────────────────────────────
#[tauri::command]
async fn login(state: State<'_, DbState>, user: String, pass: String) -> Result<bool, String> {
    let users_from_server = fetch_users_from_server().await;
    let conn = state.0.lock().unwrap();

    if let Some(users) = users_from_server {
        let count = save_users_to_sqlite(&conn, users);
        println!("✅ Sincronizados: {} usuarios", count);
    }

    let result = conn.query_row(
        "SELECT password_hash FROM users WHERE username = ?1",
        params![user],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(password_hash) => {
            let input_hash = hash_md5(&pass);
            Ok(input_hash == password_hash)
        }
        Err(_) => Ok(false),
    }
}

// ─── COMANDO SINCRONIZAR ──────────────────────────────────────────────────────
#[tauri::command]
async fn sincronizar(state: State<'_, DbState>) -> Result<String, String> {
    let users = fetch_users_from_server()
        .await
        .ok_or_else(|| "No se pudo conectar al servidor".to_string())?;

    let conn = state.0.lock().unwrap();
    let count = save_users_to_sqlite(&conn, users);

    Ok(format!("✅ {} usuarios sincronizados", count))
}

#[tauri::command]
fn obtener_inventario_local(state: State<'_, DbState>) -> Result<Vec<Equipo>, String> {
    let conn = state.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT codigo_rfid, item, numero_serie, descripcion, marca, 
modelo, categoria, cantidad, almacen, permiso_salida, estado_ubicacion FROM equipos_glef").map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok(Equipo {
            codigo_rfid: row.get(0)?,
            item: row.get(1)?,
            numero_serie: row.get(2)?,
            descripcion: row.get(3)?,
            marca: row.get(4)?,
            modelo: row.get(5)?,
            categoria: row.get(6)?,
            cantidad: row.get(7)?,
            almacen: row.get(8)?,
            permiso_salida: row.get(9).ok(),
            estado_ubicacion: row.get(10).ok(),
            status: "missing".to_string(),
        })
    }).map_err(|e| e.to_string())?;

    let mut equipos = Vec::new();
    for row in rows {
        equipos.push(row.map_err(|e| e.to_string())?);
    }
    Ok(equipos)
}

#[tauri::command]
async fn sincronizar_inventario(state: State<'_, DbState>) -> Result<String, String> {
    let inventory = fetch_inventory_from_server().await
        .ok_or_else(|| "No se pudo conectar al servidor MSSQL".to_string())?;

    let conn = state.0.lock().unwrap();
    let count = save_inventory_to_sqlite(&conn, inventory);

    Ok(format!("✅ {} equipos sincronizados", count))
}

#[tauri::command]
async fn iniciar_lectura(
    app: AppHandle,
    rfid_state: State<'_, RfidState>,
    tx_state: State<'_, RfidTxState>,
    antena: u8 // 0 para Pin 1 (Inventario), 1 para Pin 2 (Puerta)
) -> Result<(), String> {
    *rfid_state.0.lock().unwrap() = true;

    let stream = TcpStream::connect("192.168.1.180:5002")
        .await
        .map_err(|e| format!("Error conectando: {}", e))?;
    
    stream.set_nodelay(true).map_err(|e| format!("Error set_nodelay: {}", e))?;
    
    let (mut read_half, mut write_half) = stream.into_split();
    
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(32);
    *tx_state.0.lock().unwrap() = Some(tx.clone());

    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            if write_half.write_all(&cmd).await.is_err() {
                break;
            }
        }
    });
    
    println!("✅ Conectado al lector RFID");
    app.emit("rfid_estado", "conectado").unwrap();

    let _ = tx.send(build_command(0xA0, &[0x00])).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let _ = tx.send(build_command(0x60, &[0x01])).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let _ = tx.send(build_command(0xB0, &[0x1F])).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    
    // Aquí enviamos el comando 0x82 de inventario con el puerto 4 (0x03)
    let _ = tx.send(build_command(0x82, &[0x03, 0x00])).await;

    println!("📡 Inventario iniciado (0x82) usando Puerto 4");

    let (tag_tx, mut tag_rx) = mpsc::channel::<(String, u8, Vec<u8>)>(10000);
    
    let app_clone = app.clone();
    
    tokio::spawn(async move {
        let mut batch_buffer: Vec<String> = Vec::new();
        let mut contador_por_segundo = 0;
        let mut contador_total: u32 = 0;
        let mut ultimo_flush = std::time::Instant::now();
        let mut ultimo_log = std::time::Instant::now();
        let mut last_emitted: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        let mut last_db_insert: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();

        while let Some((candidate, antenna_id, frame)) = tag_rx.recv().await {
            let now = std::time::Instant::now();
            contador_por_segundo += 1;
            contador_total += 1;

            if ultimo_log.elapsed() >= Duration::from_secs(1) {
                println!("⚡ {} lecturas/segundo", contador_por_segundo);
                let _ = app_clone.emit("contador_total", serde_json::json!({ "total": contador_total }));
                contador_por_segundo = 0;
                ultimo_log = std::time::Instant::now();
            }

            let should_emit = match last_emitted.get(&candidate) {
                Some(&time) => now.duration_since(time).as_millis() > 1000,
                None => true,
            };

            if should_emit {
                last_emitted.insert(candidate.clone(), now);
                let epc_bytes = hex::decode(&candidate).unwrap_or_default();
                let candidate_ascii = String::from_utf8_lossy(&epc_bytes).into_owned();
                
                // Imprimir trama cruda para diagnóstico de protocolo
                println!("🔍 TRAMA CRUDA: {:02X?}", frame);
                println!("📦 TAG: {} (Byte 5: {}, ASCII: {})", candidate, frame[5], candidate_ascii);
                
                // Emitimos SIEMPRE la antena 1 temporalmente para que los dashboards no estén bloqueados
                app_clone.emit("tag_leido", serde_json::json!({ 
                    "epc": candidate, 
                    "epc_ascii": candidate_ascii,
                    "antena": 1 // Hardcodeado temporalmente
                })).unwrap();
            }

            let should_insert_db = match last_db_insert.get(&candidate) {
                Some(&time) => now.duration_since(time).as_millis() > 1000,
                None => true,
            };

            if should_insert_db {
                last_db_insert.insert(candidate.clone(), now);
                batch_buffer.push(candidate.clone());
            }

            if batch_buffer.len() >= 30 || (ultimo_flush.elapsed() >= Duration::from_secs(2) && !batch_buffer.is_empty()) {
                let batch_to_insert = batch_buffer.clone();
                batch_buffer.clear();
                ultimo_flush = std::time::Instant::now();
                
                let app_db = app_clone.clone();
                tokio::task::spawn_blocking(move || {
                    let db_state = app_db.state::<DbState>();
                    let mut conn = db_state.0.lock().unwrap();
                    guardar_epcs_batch(&mut conn, &batch_to_insert);
                });
            }
        }
        
        if !batch_buffer.is_empty() {
            let app_db = app_clone.clone();
            tokio::task::spawn_blocking(move || {
                let db_state = app_db.state::<DbState>();
                let mut conn = db_state.0.lock().unwrap();
                guardar_epcs_batch(&mut conn, &batch_buffer);
            });
        }
        println!("🛑 Tarea de procesamiento finalizada - Total: {} lecturas", contador_total);
    });

    let mut buffer: Vec<u8> = Vec::new();
    let mut temp = [0u8; 4096];

    loop {
        if !*rfid_state.0.lock().unwrap() {
            app.emit("rfid_estado", "detenido").unwrap();
            break;
        }

        match tokio::time::timeout(std::time::Duration::from_millis(500), read_half.read(&mut temp)).await {
            Ok(Ok(n)) if n > 0 => {
                buffer.extend_from_slice(&temp[..n]);

                let mut i = 0;
                while i + 4 < buffer.len() {
                    if buffer[i] == 0xA5 && buffer[i + 1] == 0x5A {
                        let length = ((buffer[i + 2] as usize) << 8) | buffer[i + 3] as usize;

                        if i + length <= buffer.len() {
                            let frame = &buffer[i..i + length];

                            if frame.len() > 6 && (frame[4] == 0x83 || frame[4] == 0x89 || frame[4] == 0x82) {
                                let raw_antenna = frame[5];
                                let antenna_id = match raw_antenna {
                                    0x00 | 0x80 => 1,
                                    0x01 | 0x81 => 2,
                                    0x02 | 0x82 => 3,
                                    0x03 | 0x83 => 4,
                                    _ => raw_antenna,
                                };
                                
                                let epc_len_end = frame.len().saturating_sub(3);
                                if epc_len_end > 6 {
                                    let epc_bytes = &frame[6..epc_len_end];
                                    let candidate = hex::encode(epc_bytes);
                                    
                                    if !candidate.is_empty() && candidate.len() % 4 == 0 {
                                        // Enviamos al canal de procesamiento sin bloquear
                                        let _ = tag_tx.send((candidate, antenna_id, frame.to_vec())).await;
                                    }
                                }
                            }
                            i += length;
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }

                if i > 0 {
                    buffer.drain(0..i);
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                println!("Error de lectura TCP: {}", e);
                break;
            }
            Err(_) => {
                // Timeout, continuamos para poder chequear rfid_state
                continue;
            }
        }
        
        tokio::task::yield_now().await;
    }

    Ok(())
}

// ─── COMANDO DETENER LECTURA ──────────────────────────────────────────────────
#[tauri::command]
async fn detener_lectura(rfid_state: State<'_, RfidState>, tx_state: State<'_, RfidTxState>) -> Result<(), String> {
    *rfid_state.0.lock().unwrap() = false;
    
    let tx_opt = tx_state.0.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(build_command(0x89, &[])).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    
    println!("🛑 Señal de detener enviada al lector");
    Ok(())
}

// ─── CONTROL DE PUERTA ────────────────────────────────────────────────────────
#[tauri::command]
async fn cambiar_permiso_salida(state: State<'_, DbState>, codigo_rfid: String, admin_pass: String) -> Result<String, String> {
    let conn = state.0.lock().unwrap();
    let result = conn.query_row(
        "SELECT password_hash FROM users WHERE username = 'admin'",
        [],
        |row| row.get::<_, String>(0),
    );
    let password_hash = result.unwrap_or_default();
    let input_hash = hash_md5(&admin_pass);
    if input_hash != password_hash {
        return Err("Contraseña incorrecta".to_string());
    }

    let act = conn.execute(
        "UPDATE equipos_glef SET permiso_salida = CASE WHEN permiso_salida = 1 THEN 0 ELSE 1 END WHERE LOWER(TRIM(codigo_rfid)) = LOWER(TRIM(?1))",
        params![codigo_rfid],
    ).map_err(|e| e.to_string())?;
    
    if act == 0 {
        return Err("Equipo no encontrado".to_string());
    }
    
    Ok("Permiso actualizado".to_string())
}

#[derive(serde::Serialize)]
pub struct CruceResultado {
    pub codigo_rfid: String,
    pub estado_anterior: String,
    pub nuevo_estado: String,
    pub alarma: bool,
    pub nombre_item: String,
}

#[tauri::command]
async fn registrar_cruce_puerta(state: State<'_, DbState>, tx_state: State<'_, RfidTxState>, codigo_rfid: String) -> Result<CruceResultado, String> {
    let conn = state.0.lock().unwrap();
    
    let eq = conn.query_row(
        "SELECT estado_ubicacion, permiso_salida, item FROM equipos_glef WHERE LOWER(TRIM(codigo_rfid)) = LOWER(TRIM(?1)) OR LOWER(TRIM(?1)) LIKE '%' || LOWER(TRIM(codigo_rfid)) || '%'",
        params![codigo_rfid.clone()],
        |row| {
            let estado: String = row.get(0).unwrap_or_else(|_| "En Oficina".to_string());
            let permiso: bool = row.get(1).unwrap_or(false);
            let nombre: String = row.get(2).unwrap_or_else(|_| "Desconocido".to_string());
            Ok((estado, permiso, nombre))
        }
    ).map_err(|e| {
        println!("❌ Error en query_row de registrar_cruce_puerta para {}: {}", codigo_rfid, e);
        e.to_string()
    })?;

    let estado_anterior = eq.0;
    let permiso_salida = eq.1;
    let nombre_item = eq.2;

    let nuevo_estado = if estado_anterior == "En Oficina" {
        "Afuera".to_string()
    } else {
        "En Oficina".to_string()
    };

    let alarma = nuevo_estado == "Afuera" && !permiso_salida;

    if alarma {
        let tx_opt = tx_state.0.lock().unwrap().clone();
        tokio::spawn(async move {
            activar_alarma_fisica(tx_opt).await;
        });
    }

    conn.execute(
        "UPDATE equipos_glef SET estado_ubicacion = ?1 WHERE LOWER(TRIM(codigo_rfid)) = LOWER(TRIM(?2)) OR LOWER(TRIM(?2)) LIKE '%' || LOWER(TRIM(codigo_rfid)) || '%'",
        params![nuevo_estado, codigo_rfid],
    ).map_err(|e| e.to_string())?;

    Ok(CruceResultado {
        codigo_rfid,
        estado_anterior,
        nuevo_estado,
        alarma,
        nombre_item,
    })
}

// ─── ENTRY POINT ─────────────────────────────────────────────────────────────
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app
                .path()
                .app_data_dir()
                .expect("Error obteniendo app_data_dir")
                .join("app.db");

            println!("📂 Base de datos en: {:?}", db_path);

            std::fs::create_dir_all(db_path.parent().unwrap())
                .expect("Error creando directorio");

            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            init_db(&conn);

            app.manage(DbState(Mutex::new(conn)));
            app.manage(RfidState(Mutex::new(false)));
            app.manage(RfidTxState(Mutex::new(None)));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            saludar, 
            login, 
            sincronizar, 
            obtener_inventario_local, 
            sincronizar_inventario,
            iniciar_lectura,
            detener_lectura,
            cambiar_permiso_salida,
            registrar_cruce_puerta
        ])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}
