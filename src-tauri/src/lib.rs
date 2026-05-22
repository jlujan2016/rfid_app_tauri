use rusqlite::{Connection, params};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

// ─── ESTADOS GLOBALES ─────────────────────────────────────────────────────────
pub struct DbState(pub Mutex<Connection>);
pub struct RfidState(pub Mutex<bool>);

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

         CREATE TABLE IF NOT EXISTS LecturasRFID (
            Id           INTEGER PRIMARY KEY AUTOINCREMENT,
            EPC          TEXT NOT NULL,
            FechaLectura TEXT NOT NULL DEFAULT (datetime('now')),
            sincronizado INTEGER NOT NULL DEFAULT 0
         );
         
         CREATE INDEX IF NOT EXISTS idx_epc ON LecturasRFID(EPC);
         CREATE INDEX IF NOT EXISTS idx_fecha ON LecturasRFID(FechaLectura);
         CREATE INDEX IF NOT EXISTS idx_sincronizado ON LecturasRFID(sincronizado);",
    )
    .expect("Error creando tablas");

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

// ─── GUARDAR BATCH DE EPCs EN SQLITE ──────────────────────────────────────────
fn guardar_epcs_batch(conn: &mut Connection, epcs: &[String]) {
    if epcs.is_empty() {
        return;
    }
    
    let tx = conn.transaction().expect("Error iniciando transacción");
    for epc in epcs {
        tx.execute(
            "INSERT INTO LecturasRFID (EPC, FechaLectura, sincronizado)
             VALUES (?1, datetime('now'), 0)",
            params![epc],
        ).ok();
    }
    tx.commit().expect("Error commit transacción");
    println!("💾 Batch SQLite: {} registros", epcs.len());
}

// ─── GUARDAR EPC EN SQL SERVER ────────────────────────────────────────────────
async fn guardar_epc_server(epc: String) {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(_) => {
            println!("⚠️ Sin SQL Server: {}", epc);
            return;
        }
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(e) => {
            println!("⚠️ Error SQL Server: {}", e);
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

    match client.execute(query, &[&epc.as_str()]).await {
        Ok(r) => {
            if r.total() > 0 {
                println!("💾 SQL Server: {}", epc);
            }
        }
        Err(e) => println!("❌ SQL Server ERROR: {}", e),
    }
}

// ─── SINCRONIZAR EPCs PENDIENTES CON SQL SERVER (BACKGROUND) ─────────────────
async fn sincronizar_rfid_pendiente(db_state: tauri::State<'_, DbState>) {
    use tokio::time;
    let mut interval = time::interval(Duration::from_secs(30)); // Cada 30 segundos
    
    loop {
        interval.tick().await;
        
        // Obtener EPCs no sincronizados (sincronizado = 0)
        let epcs_pendientes: Vec<String> = {
            let conn = db_state.0.lock().unwrap();
            
            // Preparar la consulta
            let mut stmt = match conn.prepare(
                "SELECT EPC FROM LecturasRFID 
                 WHERE sincronizado = 0 
                 ORDER BY Id ASC
                 LIMIT 50"
            ) {
                Ok(stmt) => stmt,
                Err(e) => {
                    println!("⚠️ Error preparando consulta: {}", e);
                    continue;
                }
            };
            
            // Ejecutar la consulta y recolectar resultados
            let epcs: Vec<String> = match stmt.query_map([], |row| row.get(0)) {
                Ok(rows) => {
                    let mut result = Vec::new();
                    for row in rows {
                        if let Ok(epc) = row {
                            result.push(epc);
                        }
                    }
                    result
                }
                Err(e) => {
                    println!("⚠️ Error consultando EPCs pendientes: {}", e);
                    continue;
                }
            };
            
            epcs
        };
        
        if !epcs_pendientes.is_empty() {
            println!("🔄 Sincronizando {} EPCs con SQL Server...", epcs_pendientes.len());
            
            let mut sincronizados = 0;
            for epc in epcs_pendientes {
                // Enviar a SQL Server
                guardar_epc_server(epc.clone()).await;
                
                // Marcar como sincronizado en SQLite
                let conn = db_state.0.lock().unwrap();
                if let Err(e) = conn.execute(
                    "UPDATE LecturasRFID SET sincronizado = 1 WHERE EPC = ?1",
                    params![epc]
                ) {
                    println!("⚠️ Error marcando EPC {} como sincronizado: {}", epc, e);
                } else {
                    sincronizados += 1;
                }
            }
            
            println!("✅ {} EPCs sincronizados con SQL Server", sincronizados);
        }
    }
}


// ─── COMANDO SINCRONIZAR RFID MANUAL ──────────────────────────────────────────
#[tauri::command]
async fn sincronizar_rfid_manual(db_state: State<'_, DbState>) -> Result<String, String> {
    // Obtener EPCs no sincronizados
    let epcs_pendientes: Vec<String> = {
        let conn = db_state.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT EPC FROM LecturasRFID 
             WHERE sincronizado = 0 
             ORDER BY Id ASC
             LIMIT 100"
        ).map_err(|e| format!("Error preparando consulta: {}", e))?;
        
        let rows = stmt.query_map([], |row| row.get(0))
            .map_err(|e| format!("Error consultando: {}", e))?;
        
        let mut result = Vec::new();
        for row in rows {
            if let Ok(epc) = row {
                result.push(epc);
            }
        }
        result
    };
    
    if epcs_pendientes.is_empty() {
        return Ok("📭 No hay datos pendientes por sincronizar".to_string());
    }
    
    println!("🔄 Sincronizando manualmente {} EPCs...", epcs_pendientes.len());
    
    let mut sincronizados = 0;
    for epc in epcs_pendientes {
        guardar_epc_server(epc.clone()).await;
        
        let conn = db_state.0.lock().unwrap();
        conn.execute(
            "UPDATE LecturasRFID SET sincronizado = 1 WHERE EPC = ?1",
            params![epc]
        ).map_err(|e| format!("Error actualizando: {}", e))?;
        
        sincronizados += 1;
    }
    
    Ok(format!("✅ {} EPCs sincronizados con SQL Server", sincronizados))
}
// ─── EXTRAER EPC UNIVERSAL (COMO EL PRIMER PROYECTO) ─────────────────────────
fn extraer_epc_universal(payload: &[u8]) -> Option<String> {
    // Método exactamente como el primer proyecto
    if payload.len() < 6 {
        return None;
    }
    
    // El EPC siempre empieza en byte[2]
    // Los ultimos 4 bytes siempre son RSSI/metadata → los ignoramos
    let epc_start = 2;
    let epc_end = payload.len() - 4;
    
    if epc_start >= epc_end {
        return None;
    }
    
    let epc = hex::encode(&payload[epc_start..epc_end]);
    
    // Validar que no sea vacío
    if epc.is_empty() {
        None
    } else {
        Some(epc)
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

// ─── COMANDO INICIAR LECTURA RFID - ULTRA RÁPIDO ──────────────────────────────
// ─── COMANDO INICIAR LECTURA RFID - ULTRA RÁPIDO (CONTADOR CORREGIDO) ─────────
#[tauri::command]
async fn iniciar_lectura(
    app: AppHandle,
    db_state: State<'_, DbState>,
    rfid_state: State<'_, RfidState>,
) -> Result<(), String> {
    *rfid_state.0.lock().unwrap() = true;

    let mut stream = TcpStream::connect("192.168.1.180:8888")
        .await
        .map_err(|e| format!("Error conectando: {}", e))?;
    
    stream.set_nodelay(true).map_err(|e| format!("Error set_nodelay: {}", e))?;
    
    println!("✅ Conectado al lector RFID");
    app.emit("rfid_estado", "conectado").unwrap();

    // Desactivar beeper
    stream.write_all(&build_command(0xA0, &[0x00])).await.ok();
    tokio::time::sleep(Duration::from_millis(30)).await;
    
    // Modo lector
    stream.write_all(&build_command(0x60, &[0x01])).await
        .map_err(|e| format!("Error modo lector: {}", e))?;
    tokio::time::sleep(Duration::from_millis(30)).await;
    
    // Potencia máxima (1 metro)
    stream.write_all(&build_command(0xB0, &[0x1F])).await.ok();
    tokio::time::sleep(Duration::from_millis(30)).await;
    
    // Iniciar inventario continuo
    stream.write_all(&build_command(0x82, &[0x00, 0x00])).await
        .map_err(|e| format!("Error inventario: {}", e))?;

    println!("📡 Inventario iniciado - ULTRA RÁPIDO");

    let mut buffer: Vec<u8> = Vec::new();
    let mut temp = [0u8; 2048];
    let mut batch_buffer: Vec<String> = Vec::with_capacity(30);
    let mut ultimo_log = std::time::Instant::now();
    let mut contador_por_segundo = 0;
    let mut contador_total: u32 = 0;  // ← CONTADOR TOTAL CORREGIDO
    let mut ultimo_flush = std::time::Instant::now();
     // Cache para evitar duplicados (como en el primer proyecto)
    let mut cache: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
    loop {
        if !*rfid_state.0.lock().unwrap() {
            if !batch_buffer.is_empty() {
                let mut conn = db_state.0.lock().unwrap();
                guardar_epcs_batch(&mut conn, &batch_buffer);
            }
            println!("🛑 Lectura detenida - Total: {} lecturas", contador_total);
            app.emit("rfid_estado", "detenido").unwrap();
            break;
        }

                // Limpiar cache cada minuto
        if ultimo_log.elapsed() >= Duration::from_secs(60) {
            cache.retain(|_, timestamp| timestamp.elapsed() < Duration::from_secs(2));
        }

        match stream.read(&mut temp).await {
            Ok(n) if n > 0 => {
                buffer.extend_from_slice(&temp[..n]);

                let mut i = 0;
                while i + 4 < buffer.len() {
                    if buffer[i] == 0xA5 && buffer[i + 1] == 0x5A {
                        let length = ((buffer[i + 2] as usize) << 8) | buffer[i + 3] as usize;

                        if i + length <= buffer.len() {
                            let frame = &buffer[i..i + length];

                            if frame.len() > 6 && frame[4] == 0x83 {
                                let payload = &frame[5..frame.len().saturating_sub(2)];
                                
                                // USAR EL EXTRACTOR UNIVERSAL
                                if let Some(epc) = extraer_epc_universal(payload) {
                                    let now = std::time::Instant::now();
                                    
                                    // Verificar si ya vimos este EPC recientemente (menos de 2 segundos)
                                    if let Some(last_time) = cache.get(&epc) {
                                        if now.duration_since(*last_time) < Duration::from_secs(2) {
                                            continue; // Ignorar duplicado
                                        }
                                    }
                                    
                                    // Insertar en cache
                                    cache.insert(epc.clone(), now);
                                    
                                    // Contar estadísticas
                                    contador_por_segundo += 1;
                                    contador_total += 1;
                                    
                                    // Mostrar tasa de lectura cada segundo
                                    if ultimo_log.elapsed() >= Duration::from_secs(1) {
                                        println!("⚡ {} lecturas/segundo (total únicas: {})", 
                                            contador_por_segundo, contador_total);
                                        app.emit("lecturas_por_segundo", contador_por_segundo).unwrap();
                                        contador_por_segundo = 0;
                                        ultimo_log = std::time::Instant::now();
                                    }
                                    
                                    // Emitir evento al frontend
                                    app.emit("tag_leido", &epc).unwrap();
                                    app.emit("contador_total", &contador_total).unwrap();
                                    
                                    // Acumular para batch
                                    batch_buffer.push(epc);
                                    
                                    // Guardar batch cuando alcanza 30 o pasaron 3 segundos
                                    if batch_buffer.len() >= 30 || ultimo_flush.elapsed() >= Duration::from_secs(3) {
                                        let mut conn = db_state.0.lock().unwrap();
                                        guardar_epcs_batch(&mut conn, &batch_buffer);
                                        println!("💾 Guardados {} EPCs en SQLite", batch_buffer.len());
                                        batch_buffer.clear();
                                        ultimo_flush = std::time::Instant::now();
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
            Ok(_) => continue,
            Err(e) => {
                println!("Error de lectura: {}", e);
                break;
            }
        }
        
        tokio::task::yield_now().await;
    }

    Ok(())
}

// ─── COMANDO DETENER LECTURA ──────────────────────────────────────────────────
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
                .expect("Error creando directorio");

            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            init_db(&conn);

            let db_state = DbState(Mutex::new(conn));
            app.manage(db_state);
            app.manage(RfidState(Mutex::new(false)));

            // 🔥 INICIAR TAREA DE SINCRONIZACIÓN EN BACKGROUND
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let db_state = app_handle.state::<DbState>();
                sincronizar_rfid_pendiente(db_state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            saludar,
            login,
            sincronizar,
            sincronizar_rfid_manual,
            iniciar_lectura,
            detener_lectura
        ])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}