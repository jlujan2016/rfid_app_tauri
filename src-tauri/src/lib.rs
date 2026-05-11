use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
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
    pub status: String,
}

// ─── ESTADOS GLOBALES ─────────────────────────────────────────────────────────
pub struct DbState(pub Mutex<Connection>);
pub struct RfidState(pub Mutex<bool>); // true = leyendo

// ─── HELPER MD5 ──────────────────────────────────────────────────────────────
fn hash_md5(input: &str) -> String {
    format!("{:x}", md5::compute(input))
}

// ─── COMANDO RFID ─────────────────────────────────────────────────────────────
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
        "CREATE TABLE IF NOT EXISTS users (
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
            updated_at    TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS LecturasRFID (
            Id           INTEGER PRIMARY KEY AUTOINCREMENT,
            EPC          TEXT NOT NULL,
            FechaLectura TEXT NOT NULL DEFAULT (datetime('now')),
            sincronizado INTEGER NOT NULL DEFAULT 0
        );"
    )
    .expect("Error creando tablas");

    // Intentamos agregar la columna si la tabla ya existía de antes
    let _ = conn.execute("ALTER TABLE equipos_glef ADD COLUMN almacen TEXT", []);

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
        println!("✅ Usuario admin creado con MD5");
    }
}

// ─── SINCRONIZACIÓN USUARIOS ──────────────────────────────────────────────────
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

// ─── GUARDAR EPC EN SQLITE ────────────────────────────────────────────────────
fn guardar_epc_sqlite(conn: &Connection, epc: &str) {
    match conn.execute(
        "INSERT INTO LecturasRFID (EPC, FechaLectura, sincronizado)
         VALUES (?1, datetime('now'), 0)",
        params![epc],
    ) {
        Ok(_) => println!("💾 SQLite INSERT OK: {}", epc),
        Err(e) => println!("❌ SQLite ERROR: {}", e),
    }
}

// ─── GUARDAR EPC EN SQL SERVER ────────────────────────────────────────────────
async fn guardar_epc_server(epc: &str) {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(_) => {
            println!("⚠️ Sin conexión, EPC quedó pendiente en SQLite: {}", epc);
            return;
        }
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(e) => {
            println!("⚠️ Error autenticando SQL Server: {}", e);
            return;
        }
    };

    let query = "
        INSERT INTO LecturasRFID (EPC)
        SELECT @P1
        WHERE NOT EXISTS (
            SELECT 1 FROM LecturasRFID WHERE EPC = @P1
        )
    ";

    match client.execute(query, &[&epc]).await {
        Ok(r) => {
            if r.total() > 0 {
                println!("💾 SQL Server INSERT OK: {}", epc);
            } else {
                println!("⚠️ Duplicado en SQL Server: {}", epc);
            }
        }
        Err(e) => println!("❌ SQL Server ERROR: {}", e),
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
        println!("✅ Sincronización automática: {} usuarios", count);
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

// ─── COMANDO SINCRONIZAR MANUAL ───────────────────────────────────────────────
#[tauri::command]
async fn sincronizar(state: State<'_, DbState>) -> Result<String, String> {
    let users = fetch_users_from_server()
        .await
        .ok_or_else(|| "No se pudo conectar al servidor".to_string())?;

    let conn = state.0.lock().unwrap();
    let count = save_users_to_sqlite(&conn, users);

    Ok(format!("✅ {} usuarios sincronizados correctamente", count))
}

#[tauri::command]
fn obtener_inventario_local(state: State<'_, DbState>) -> Result<Vec<Equipo>, String> {
    let conn = state.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT codigo_rfid, item, numero_serie, descripcion, marca, modelo, categoria, cantidad, almacen FROM equipos_glef").map_err(|e| e.to_string())?;
    
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
            status: "missing".to_string(),
        })
    }).map_err(|e| e.to_string())?;

    let mut equipos = Vec::new();
    for eq in rows {
        if let Ok(e) = eq {
            equipos.push(e);
        }
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

// ─── COMANDO INICIAR LECTURA RFID ─────────────────────────────────────────────
#[tauri::command]
async fn iniciar_lectura(
    app: AppHandle,
    db_state: State<'_, DbState>,
    rfid_state: State<'_, RfidState>,
) -> Result<(), String> {
    // Marcar como activo
    *rfid_state.0.lock().unwrap() = true;

    // Conectar al lector RFID
    let mut stream = TcpStream::connect("192.168.1.180:8888")
        .await
        .map_err(|e| format!("Error conectando al lector RFID: {}", e))?;

    println!("✅ Conectado al lector RFID");
    app.emit("rfid_estado", "conectado").unwrap();

    // Configurar modo lector e iniciar inventario
    stream
        .write_all(&build_command(0x60, &[0x01]))
        .await
        .map_err(|e| format!("Error enviando comando: {}", e))?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    stream
        .write_all(&build_command(0x82, &[0x00, 0x00]))
        .await
        .map_err(|e| format!("Error iniciando inventario: {}", e))?;

    println!("📡 Inventario iniciado");

    let mut buffer: Vec<u8> = Vec::new();
    let mut temp = [0u8; 1024];
    let mut cache: HashMap<String, Instant> = HashMap::new();

    loop {
        // Verificar si se pidió detener
        if !*rfid_state.0.lock().unwrap() {
            println!("🛑 Lectura RFID detenida");
            app.emit("rfid_estado", "detenido").unwrap();
            break;
        }

        // Leer datos del lector con timeout
        let n = match tokio::time::timeout(
            Duration::from_millis(100),
            stream.read(&mut temp),
        )
        .await
        {
            Ok(Ok(n)) => n,
            Ok(Err(_)) => break,
            Err(_) => continue, // timeout, volver a revisar estado
        };

        if n == 0 {
            continue;
        }

        buffer.extend_from_slice(&temp[..n]);

        let mut i = 0;
        while i + 4 < buffer.len() {
            if buffer[i] != 0xA5 || buffer[i + 1] != 0x5A {
                i += 1;
                continue;
            }

            let length = ((buffer[i + 2] as usize) << 8) | buffer[i + 3] as usize;

            if i + length > buffer.len() {
                break;
            }

            let frame = &buffer[i..i + length];

            if frame.len() > 6 && frame[4] == 0x83 {
                let payload = &frame[5..frame.len().saturating_sub(2)];
                let mut found = String::new();

                for window in payload.windows(4) {
                    let candidate = hex::encode(window);
                    if candidate.starts_with("0041") || candidate.starts_with("0040") {
                        found = candidate;
                        break;
                    }
                }

                if !found.is_empty() {
                    let now = Instant::now();
                    if let Some(last) = cache.get(&found) {
                        if now.duration_since(*last) < Duration::from_secs(2) {
                            i += length;
                            continue;
                        }
                    }
                    cache.insert(found.clone(), now);

                    println!("📦 TAG: {}", found);

                    // 1. Guardar en SQLite
                    {
                        let conn = db_state.0.lock().unwrap();
                        guardar_epc_sqlite(&conn, &found);
                    }

                    // 2. Intentar guardar en SQL Server
                    guardar_epc_server(&found).await;

                    // 3. Emitir evento al frontend
                    app.emit("tag_leido", &found).unwrap();
                }
            }

            i += length;
        }

        buffer.drain(0..i);
    }

    Ok(())
}

// ─── COMANDO DETENER LECTURA RFID ─────────────────────────────────────────────
#[tauri::command]
fn detener_lectura(rfid_state: State<'_, RfidState>) {
    *rfid_state.0.lock().unwrap() = false;
    println!("🛑 Señal de detener enviada");
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
                .expect("Error creando directorio de la BD");

            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            init_db(&conn);
            app.manage(DbState(Mutex::new(conn)));
            app.manage(RfidState(Mutex::new(false)));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            saludar, 
            login, 
            sincronizar, 
            obtener_inventario_local, 
            sincronizar_inventario,
            iniciar_lectura,
            detener_lectura
        ])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}