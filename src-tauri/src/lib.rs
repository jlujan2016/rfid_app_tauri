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
            Antena       INTEGER, 
            FechaLectura TEXT NOT NULL DEFAULT (datetime('now')),
            sincronizado INTEGER NOT NULL DEFAULT 0
         );
         
         CREATE INDEX IF NOT EXISTS idx_epc ON LecturasRFID(EPC);
         CREATE INDEX IF NOT EXISTS idx_fecha ON LecturasRFID(FechaLectura);
         CREATE INDEX IF NOT EXISTS idx_sincronizado ON LecturasRFID(sincronizado);
         CREATE TABLE IF NOT EXISTS EQUIPOS_GLEF (
    Id              INTEGER PRIMARY KEY AUTOINCREMENT,
    CODIGO_RFID     TEXT NOT NULL UNIQUE,
    DESCRIPCION     TEXT,
    MARCA           TEXT,
    MODELO          TEXT,
    CATEGORIA       TEXT,
    TIPO_PRODUCTO   TEXT,
    ESTADO          TEXT,
    ultima_sync     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS LecturasRFID_Salidas (
    Id           INTEGER PRIMARY KEY AUTOINCREMENT,
    EPC          TEXT NOT NULL,
    Antena       INTEGER,
    FechaLectura TEXT NOT NULL DEFAULT (datetime('now')),
    Alerta       INTEGER NOT NULL DEFAULT 0,
    sincronizado INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_rfid_codigo ON EQUIPOS_GLEF(CODIGO_RFID);
CREATE INDEX IF NOT EXISTS idx_salidas_epc ON LecturasRFID_Salidas(EPC);",
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
fn guardar_epcs_batch(conn: &mut Connection, epcs: &[(String, u8)]) {
    if epcs.is_empty() {
        return;
    }
    
    let tx = conn.transaction().expect("Error iniciando transacción");
    for (epc, antena) in epcs {
        tx.execute(
            "INSERT INTO LecturasRFID (EPC, Antena, FechaLectura, sincronizado)
             VALUES (?1, ?2, datetime('now'), 0)",
            params![epc, *antena as i32],
        ).ok();
    }
    tx.commit().expect("Error commit transacción");
    println!("💾 Batch SQLite: {} registros", epcs.len());
}

// ─── GUARDAR EPC EN SQL SERVER ────────────────────────────────────────────────
async fn guardar_epc_server(epc: String, antena: u8) {
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
        INSERT INTO LecturasRFID (EPC, Antena)
        SELECT @P1, @P2
        WHERE NOT EXISTS (
            SELECT 1 FROM LecturasRFID WHERE EPC = @P1
        )
    ";

    let antena_i32 = antena as i32;
    match client.execute(query, &[&epc.as_str(), &antena_i32]).await {
        Ok(r) => {
            if r.total() > 0 {
                println!("💾 SQL Server: {}  antena: {}", epc, antena);
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
        let epcs_pendientes: Vec<(String, u8)> = {
            let conn = db_state.0.lock().unwrap();
            // Preparar la consulta
            let mut stmt = match conn.prepare(
                "SELECT EPC, Antena FROM LecturasRFID 
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
            let epcs: Vec<(String, u8)> = match stmt.query_map([], |row| {
                let epc: String = row.get(0)?;
                let antena: i32 = row.get(1).unwrap_or(1);
                Ok((epc, antena as u8))
            }) {
                Ok(rows) => {
                    let mut result = Vec::new();
                    for row in rows {
                        if let Ok(par) = row {
                            result.push(par);
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
            for (epc, antena) in epcs_pendientes {
                // Enviar a SQL Server con antena
                guardar_epc_server(epc.clone(), antena).await;
                
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
    let epcs_pendientes: Vec<(String, u8)> = {
        let conn = db_state.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT EPC, Antena FROM LecturasRFID 
             WHERE sincronizado = 0 
             ORDER BY Id ASC
             LIMIT 100"
        ).map_err(|e| format!("Error preparando consulta: {}", e))?;
        
        let rows = stmt.query_map([], |row| {
            let epc: String = row.get(0)?;
            let antena: i32 = row.get(1).unwrap_or(1);
            Ok((epc, antena as u8))
        }).map_err(|e| format!("Error consultando: {}", e))?;
        
        let mut result = Vec::new();
        for row in rows {
            if let Ok(par) = row {
                result.push(par);
            }
        }
        result
    };
    
    if epcs_pendientes.is_empty() {
        return Ok("📭 No hay datos pendientes por sincronizar".to_string());
    }
    
    println!("🔄 Sincronizando manualmente {} EPCs...", epcs_pendientes.len());
    
    let mut sincronizados = 0;
    for (epc, antena) in epcs_pendientes {
        guardar_epc_server(epc.clone(), antena).await;
        
        let conn = db_state.0.lock().unwrap();
        conn.execute(
            "UPDATE LecturasRFID SET sincronizado = 1 WHERE EPC = ?1",
            params![epc]
        ).map_err(|e| format!("Error actualizando: {}", e))?;
        
        sincronizados += 1;
    }
    
    Ok(format!("✅ {} EPCs sincronizados con SQL Server", sincronizados))
}
// ─── ESTRUCTURA LECTURA RFID ──────────────────────────────────────────────────
#[derive(Clone)]
struct LecturaRfid {
    epc:    String,
    antena: u8,
}
// ─── EXTRAER EPC UNIVERSAL (COMO EL PRIMER PROYECTO) ─────────────────────────
fn extraer_epc_universal(payload: &[u8]) -> Option<LecturaRfid> {
    // Método exactamente como el primer proyecto
    if payload.len() < 6 {
        return None;
    }
    let antena    = payload[payload.len() - 1]; // 01, 02, 03 o 04
    // validar rango 1 al 4
    if antena < 1 || antena > 4 {
        return None;
    }

    // El EPC siempre empieza en byte[2]
    // Los ultimos 4 bytes siempre son RSSI/metadata → los ignoramos
    let epc_start = 2;
    let epc_end = payload.len() - 3;
    
    if epc_start >= epc_end {
        return None;
    }
    
    let epc = hex::encode(&payload[epc_start..epc_end]);
    
    // Validar que no sea vacío
    if epc.is_empty() {
        None
    } else {
        Some(LecturaRfid { epc, antena })
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
// ─── OBTENER EQUIPOS DESDE SQL SERVER ────────────────────────────────────────
async fn sync_equipos_desde_server() -> Result<Vec<(String, String, String, String, String, String, String)>, String> {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = TcpStream::connect(config.get_addr())
        .await
        .map_err(|e| format!("Sin conexión: {}", e))?;

    let mut client = Client::connect(config, tcp.compat_write())
        .await
        .map_err(|e| format!("Error autenticando: {}", e))?;

    let stream = client
        .query(
            "SELECT CODIGO_RFID, DESCRIPCION, MARCA, MODELO,
                    CATEGORIA, [TIPO DE PRODUCTO], ESTADO
             FROM EQUIPOS_GLEF
             WHERE CODIGO_RFID IS NOT NULL",
            &[],
        )
        .await
        .map_err(|e| format!("Error consultando: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Error leyendo filas: {}", e))?;

    let equipos = rows.iter().filter_map(|row| {
        let codigo_rfid:   &str = row.get(0)?;
        let descripcion:   &str = row.get(1).unwrap_or("");
        let marca:         &str = row.get(2).unwrap_or("");
        let modelo:        &str = row.get(3).unwrap_or("");
        let categoria:     &str = row.get(4).unwrap_or("");
        let tipo_producto: &str = row.get(5).unwrap_or("");
        let estado:        &str = row.get(6).unwrap_or("");
        Some((
            codigo_rfid.to_string(),
            descripcion.to_string(),
            marca.to_string(),
            modelo.to_string(),
            categoria.to_string(),
            tipo_producto.to_string(),
            estado.to_string(),
        ))
    }).collect();

    Ok(equipos)
}

// ─── VERIFICAR SI EPC ES USO INTERNO ──────────────────────────────────────────
fn es_uso_interno(conn: &Connection, epc: &str) -> Option<String> {
    // Retorna la descripcion del equipo si es Uso Interno, None si no
    let result = conn.query_row(
        "SELECT DESCRIPCION FROM EQUIPOS_GLEF 
         WHERE CODIGO_RFID = ?1 
         AND TIPO_PRODUCTO = 'Uso Interno'",
        params![epc],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(descripcion) => Some(descripcion),
        Err(_)          => None,
    }
}

// ─── COMANDO SINCRONIZAR ──────────────────────────────────────────────────────
#[tauri::command]
async fn sincronizar(state: State<'_, DbState>) -> Result<String, String> {
    // 1. Obtener datos del servidor (sin tener el lock de SQLite)
    let users   = fetch_users_from_server().await
        .ok_or_else(|| "No se pudo conectar al servidor".to_string())?;
    let equipos = sync_equipos_desde_server().await?;
    // DEBUG temporal
        println!("DEBUG equipos recibidos: {}", equipos.len());
        for e in &equipos {
            println!("  → {:?}", e);
        }

    // 2. Guardar en SQLite (lock solo aquí, sin await dentro)
    let conn = state.0.lock().unwrap();

    let count_users = save_users_to_sqlite(&conn, users);

    let mut count_equipos = 0;
    for (codigo_rfid, descripcion, marca, modelo, categoria, tipo_producto, estado) in &equipos {
        if conn.execute(
            "INSERT INTO EQUIPOS_GLEF
                (CODIGO_RFID, DESCRIPCION, MARCA, MODELO, CATEGORIA, TIPO_PRODUCTO, ESTADO, ultima_sync)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
             ON CONFLICT(CODIGO_RFID) DO UPDATE SET
                 DESCRIPCION   = excluded.DESCRIPCION,
                 MARCA         = excluded.MARCA,
                 MODELO        = excluded.MODELO,
                 CATEGORIA     = excluded.CATEGORIA,
                 TIPO_PRODUCTO = excluded.TIPO_PRODUCTO,
                 ESTADO        = excluded.ESTADO,
                 ultima_sync   = excluded.ultima_sync",
            params![codigo_rfid, descripcion, marca, modelo, categoria, tipo_producto, estado],
        ).is_ok() {
            count_equipos += 1;
        }
    }

    println!("✅ Equipos sincronizados: {}", count_equipos);

    Ok(format!("✅ {} usuarios y {} equipos sincronizados", count_users, count_equipos))
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

    let mut stream = TcpStream::connect("127.0.0.1:5084")
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
    let mut batch_buffer: Vec<(String, u8)> = Vec::with_capacity(30);
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
                                let payload = &frame[5..frame.len().saturating_sub(3)];
                                
                                // USAR EL EXTRACTOR UNIVERSAL
                                if let Some(lectura) = extraer_epc_universal(payload) {
                                    let now = std::time::Instant::now();
                                    
                                    // Verificar si ya vimos este EPC recientemente (menos de 2 segundos)
                                    if let Some(last_time) = cache.get(&lectura.epc) {
                                        if now.duration_since(*last_time) < Duration::from_secs(2) {
                                            continue; // Ignorar duplicado
                                        }
                                    }
                                    
                                    // Insertar en cache
                                    cache.insert(lectura.epc.clone(), now);
                                    
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
                                    
                                    // Emitir evento al frontend con EPC y antena
                                    app.emit("tag_leido", &lectura.epc).unwrap();
                                    app.emit("antena_activa", &lectura.antena).unwrap(); // ← nuevo evento antena
                                    app.emit("contador_total", &contador_total).unwrap();
                                    
                                    if lectura.antena == 2 {
                                        // ── Antena 2: control de salida ──────────────
                                        let conn = db_state.0.lock().unwrap();

                                        // Verificar si el equipo es Uso Interno
                                        let descripcion_alerta = es_uso_interno(&conn, &lectura.epc);
                                        let es_alerta = descripcion_alerta.is_some();

                                        // Guardar en tabla de salidas
                                        conn.execute(
                                            "INSERT INTO LecturasRFID_Salidas (EPC, Antena, FechaLectura, Alerta)
                                             VALUES (?1, ?2, datetime('now'), ?3)",
                                            params![lectura.epc, lectura.antena as i32, es_alerta as i32],
                                        ).ok();

                                        if es_alerta {
                                            let descripcion = descripcion_alerta.unwrap();
                                            println!("🚨 ALERTA USO INTERNO: {} — {}", lectura.epc, descripcion);
                                            app.emit("alerta_uso_interno", &lectura.epc).unwrap();
                                        }

                                        app.emit("tag_salida", &lectura.epc).unwrap();

                                    } else {
                                        // ── Antena 1 y otras: inventario general ─────
                                        batch_buffer.push((lectura.epc.clone(), lectura.antena));

                                        if batch_buffer.len() >= 30 || ultimo_flush.elapsed() >= Duration::from_secs(3) {
                                            let mut conn = db_state.0.lock().unwrap();
                                            guardar_epcs_batch(&mut conn, &batch_buffer);
                                            println!("💾 Guardados {} EPCs en SQLite (antena {})",
                                                batch_buffer.len(), lectura.antena);
                                            batch_buffer.clear();
                                            ultimo_flush = std::time::Instant::now();
                                        }
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