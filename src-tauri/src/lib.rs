use rusqlite::{Connection, params};
use std::sync::{Mutex, Arc};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::env;
use tauri::{AppHandle, Emitter, Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use dotenvy::dotenv;
use std::sync::atomic::{AtomicBool, Ordering};

// ─── ESTADOS GLOBALES ─────────────────────────────────────────────────────────
pub struct DbState(pub Arc<Mutex<Connection>>);
pub struct RfidState(pub Arc<Mutex<bool>>);
pub struct RelayState {
    pub tx: tokio::sync::mpsc::Sender<RelayCommand>,
}

#[derive(Clone)]
pub enum RelayCommand {
    Trigger,
}

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

// ─── SECUENCIA DE INICIALIZACIÓN (SIN CAMBIAR EL MODO DEL LECTOR) ─────────────
async fn inicializar_lector(stream: &mut TcpStream) -> Result<(), String> {
    // 1. Soft reset (limpia estado previo)
    println!("🔄 Soft reset...");
    stream.write_all(&build_command(0xA2, &[])).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // 2. Apagar beep
    println!("🔇 Beep OFF...");
    stream.write_all(&build_command(0xA0, &[0x00])).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 3. Configurar potencia (30 dBm)
    println!("📡 Configurando potencia 30dBm...");
    stream.write_all(&build_command(0x93, &[30])).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 4. INICIAR INVENTARIO CONTINUO (respetando el modo actual del lector)
    println!("🚀 Iniciando inventario continuo...");
    stream.write_all(&build_command(0x82, &[0x00, 0x00])).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    println!("✅ Lector inicializado - Inventario continuo activo");
    Ok(())
}

// ─── CONEXIÓN DEL LECTOR ──────────────────────────────────────────────────────
async fn conectar_lector() -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect("192.168.100.180:5002")
        .await
        .map_err(|e| format!("Error conectando: {}", e))?;
    
    stream.set_nodelay(true).map_err(|e| format!("Error set_nodelay: {}", e))?;
    
    inicializar_lector(&mut stream).await?;
    
    Ok(stream)
}

// ─── FORZAR REACTIVACIÓN COMPLETA DEL LECTOR ───────────────────────────────────
async fn forzar_reactivacion_completa(stream: &mut TcpStream) -> Result<(), String> {
    println!("🔄 Forzando reactivación completa...");
    
    // Detener inventario
    let _ = stream.write_all(&build_command(0x8C, &[])).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Soft reset suave
    let _ = stream.write_all(&build_command(0xA2, &[])).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Reconfigurar potencia
    let _ = stream.write_all(&build_command(0x93, &[30])).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Reiniciar inventario continuo
    stream.write_all(&build_command(0x82, &[0x00, 0x00])).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    println!("✅ Lector reactivado");
    Ok(())
}

// ─── CONTROL DEL RELAY (CONEXIONES INDEPENDIENTES) ────────────────────────────
async fn relay_on() -> bool {
    match TcpStream::connect("192.168.100.180:5002").await {
        Ok(mut s) => {
            let _ = s.set_nodelay(true);
            let _ = s.write_all(&build_command(0xA1, &[0x09, 0x00, 0x00, 0x01])).await;
            println!("🔓 RELAY ON");
            true
        }
        Err(e) => {
            println!("❌ Error en ON: {}", e);
            false
        }
    }
}

async fn relay_off() -> bool {
    match TcpStream::connect("192.168.100.180:5002").await {
        Ok(mut s) => {
            let _ = s.set_nodelay(true);
            let _ = s.write_all(&build_command(0xA1, &[0x09, 0x00, 0x00, 0x00])).await;
            println!("🔒 RELAY OFF");
            true
        }
        Err(e) => {
            println!("❌ Error en OFF: {}", e);
            false
        }
    }
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
         CREATE INDEX IF NOT EXISTS idx_salidas_epc ON LecturasRFID_Salidas(EPC);
         
         CREATE TABLE IF NOT EXISTS destinatarios_alerta (
            Id      INTEGER PRIMARY KEY AUTOINCREMENT,
            correo  TEXT NOT NULL UNIQUE,
            nombre  TEXT,
            activo  INTEGER NOT NULL DEFAULT 1
         );",
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

// ─── SINCRONIZAR RFID PENDIENTE (BACKGROUND) ─────────────────────────────────
async fn sincronizar_rfid_pendiente(db_state: Arc<Mutex<Connection>>) {
    use tokio::time;
    let mut interval = time::interval(Duration::from_secs(30));
    
    loop {
        interval.tick().await;
        
        let epcs_pendientes: Vec<(String, u8)> = {
            let conn = db_state.lock().unwrap();
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
                guardar_epc_server(epc.clone(), antena).await;
                
                let conn = db_state.lock().unwrap();
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

// ─── GUARDAR SALIDA EN SQL SERVER ─────────────────────────────────────────────
async fn guardar_salida_server(epc: String, antena: u8, alerta: bool) {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(_) => {
            println!("⚠️ Sin SQL Server para salida: {}", epc);
            return;
        }
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(e) => {
            println!("⚠️ Error SQL Server salida: {}", e);
            return;
        }
    };

    let query = "
        INSERT INTO LecturasRFID_Salidas (EPC, Antena, Alerta)
        VALUES (@P1, @P2, @P3)
    ";

    let antena_i32 = antena as i32;
    let alerta_i32 = alerta as i32;

    match client.execute(query, &[&epc.as_str(), &antena_i32, &alerta_i32]).await {
        Ok(_) => println!("💾 Salida SQL Server: {}  alerta: {}", epc, alerta),
        Err(e) => println!("❌ Error salida SQL Server: {}", e),
    }
}

// ─── COMANDO SINCRONIZAR RFID MANUAL ──────────────────────────────────────────
#[tauri::command]
async fn sincronizar_rfid_manual(db_state: State<'_, DbState>) -> Result<String, String> {
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

// ─── EXTRAER EPC UNIVERSAL ────────────────────────────────────────────────────
fn extraer_epc_universal(payload: &[u8]) -> Option<(String, u8)> {
    if payload.len() < 6 {
        return None;
    }
    let antena = payload[payload.len() - 1];
    if antena < 1 || antena > 4 {
        return None;
    }

    let epc_start = 2;
    let epc_end = payload.len() - 3;
    
    if epc_start >= epc_end {
        return None;
    }
    
    let epc = hex::encode(&payload[epc_start..epc_end]);
    
    if epc.is_empty() {
        None
    } else {
        Some((epc, antena))
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
    let result = conn.query_row(
        "SELECT DESCRIPCION FROM EQUIPOS_GLEF 
         WHERE CODIGO_RFID = ?1 
         AND TIPO_PRODUCTO = 'Uso Interno'",
        params![epc],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(descripcion) => Some(descripcion),
        Err(_) => None,
    }
}

// ─── OBTENER DESTINATARIOS DESDE SQL SERVER ───────────────────────────────────
async fn fetch_destinatarios_from_server() -> Vec<(String, String)> {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    let tcp = match TcpStream::connect(config.get_addr()).await {
        Ok(tcp) => tcp,
        Err(_) => return vec![],
    };

    let mut client = match Client::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let stream = match client
        .query(
            "SELECT correo, ISNULL(nombre, '') FROM destinatarios_alerta 
             WHERE activo = 1",
            &[],
        )
        .await
    {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows = match stream.into_first_result().await {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    rows.iter().filter_map(|row| {
        let correo: &str = row.get(0)?;
        let nombre: &str = row.get(1)?;
        Some((correo.to_string(), nombre.to_string()))
    }).collect()
}

// ─── GUARDAR DESTINATARIOS EN SQLITE ─────────────────────────────────────────
fn save_destinatarios_to_sqlite(conn: &Connection, destinatarios: Vec<(String, String)>) -> usize {
    let mut count = 0;
    for (correo, nombre) in &destinatarios {
        if conn.execute(
            "INSERT INTO destinatarios_alerta (correo, nombre, activo)
             VALUES (?1, ?2, 1)
             ON CONFLICT(correo) DO UPDATE SET
                 nombre = excluded.nombre,
                 activo = excluded.activo",
            params![correo, nombre],
        ).is_ok() {
            count += 1;
        }
    }
    count
}

// ─── COMANDO SINCRONIZAR GENERAL ──────────────────────────────────────────────
#[tauri::command]
async fn sincronizar(state: State<'_, DbState>) -> Result<String, String> {
    let users = fetch_users_from_server().await
        .ok_or_else(|| "No se pudo conectar al servidor".to_string())?;
    let equipos = sync_equipos_desde_server().await?;
    let destinatarios = fetch_destinatarios_from_server().await;

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

    let count_destinatarios = save_destinatarios_to_sqlite(&conn, destinatarios);

    println!("✅ Usuarios: {}  Equipos: {}  Destinatarios: {}", 
        count_users, count_equipos, count_destinatarios);

    Ok(format!(
        "✅ {} usuarios, {} equipos y {} destinatarios sincronizados",
        count_users, count_equipos, count_destinatarios
    ))
}

// ─── ENVIAR EMAIL DE ALERTA ───────────────────────────────────────────────────
fn enviar_email_alerta(
    destinatarios: Vec<String>,
    epc: &str,
    descripcion: &str,
) {
    use lettre::{
        Message, SmtpTransport, Transport,
        message::header::ContentType,
        transport::smtp::authentication::Credentials,
    };

    let gmail_user = match env::var("GMAIL_USER") {
        Ok(v) => v,
        Err(_) => {
            println!("❌ GMAIL_USER no configurado en .env");
            return;
        }
    };
    let gmail_pass = match env::var("GMAIL_PASS") {
        Ok(v) => v,
        Err(_) => {
            println!("❌ GMAIL_PASS no configurado en .env");
            return;
        }
    };

    let creds = Credentials::new(gmail_user.clone(), gmail_pass);

    let mailer = match SmtpTransport::relay("smtp.gmail.com") {
        Ok(m) => m.credentials(creds).build(),
        Err(e) => {
            println!("❌ Error configurando SMTP: {}", e);
            return;
        }
    };

    let asunto = format!("🚨 ALERTA: Equipo uso interno detectado saliendo");
    let cuerpo = format!(
        "Se detectó un equipo de uso interno intentando salir.\n\n\
         EPC      : {}\n\
         Equipo   : {}\n\
         Fecha    : {}\n\n\
         Por favor tome las medidas necesarias.",
        epc,
        descripcion,
        chrono::Local::now().format("%d/%m/%Y %H:%M:%S")
    );

    for correo in &destinatarios {
        let email = match Message::builder()
            .from(gmail_user.parse().unwrap())
            .to(correo.parse().unwrap())
            .subject(&asunto)
            .header(ContentType::TEXT_PLAIN)
            .body(cuerpo.clone())
        {
            Ok(e) => e,
            Err(e) => {
                println!("❌ Error construyendo email para {}: {}", correo, e);
                continue;
            }
        };

        match mailer.send(&email) {
            Ok(_) => println!("📧 Email enviado a: {}", correo),
            Err(e) => println!("❌ Error enviando a {}: {}", correo, e),
        }
    }
}

// ─── COMANDO INICIAR LECTURA RFID (VERSIÓN DEFINITIVA) ─────────────────────────
#[tauri::command]
async fn iniciar_lectura(
    app: AppHandle,
    db_state: State<'_, DbState>,
    rfid_state: State<'_, RfidState>,
) -> Result<(), String> {
    {
        let mut activo = rfid_state.0.lock().unwrap();
        if *activo {
            return Err("El lector ya está activo".to_string());
        }
        *activo = true;
    }
    
    let db_state_clone = db_state.0.clone();
    
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, u8)>(5000);
    
    let db_state_clone_batch = db_state.0.clone();
    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(200);
        let mut timer = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                Some(lectura) = rx.recv() => {
                    batch.push(lectura);
                    if batch.len() >= 200 {
                        let mut conn = db_state_clone_batch.lock().unwrap();
                        guardar_epcs_batch(&mut conn, &batch);
                        batch.clear();
                    }
                }
                _ = timer.tick() => {
                    if !batch.is_empty() {
                        let mut conn = db_state_clone_batch.lock().unwrap();
                        guardar_epcs_batch(&mut conn, &batch);
                        batch.clear();
                    }
                }
            }
        }
    });
    
    let last_seen_global: Arc<tokio::sync::Mutex<HashMap<String, Instant>>> = Arc::new(tokio::sync::Mutex::new(HashMap::with_capacity(1000)));
    let last_seen_salidas: Arc<tokio::sync::Mutex<HashMap<String, Instant>>> = Arc::new(tokio::sync::Mutex::new(HashMap::with_capacity(200)));
    let relay_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<AtomicBool>>>> = 
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    
    let mut total_lecturas = 0u32;
    let mut stats_timer = Instant::now();
    let mut stats_count = 0;
    let mut cleanup_counter = 0u32;
    
    let mut buffer: Vec<u8> = Vec::with_capacity(8192);
    let mut temp = [0u8; 8192];
    
    // Conectar e inicializar
    let mut stream = conectar_lector().await?;
    println!("✅ Conectado y configurado");
    app.emit("rfid_estado", "conectado").unwrap();
    println!("📡 Inventario continuo activo");
    
    // Canal para señalar reconexión desde el relay
    let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel::<()>(10);
    
    while *rfid_state.0.lock().unwrap() {
        if cleanup_counter % 1000 == 0 {
            let now = Instant::now();
            {
                let mut global = last_seen_global.lock().await;
                global.retain(|_, t| now.duration_since(*t) < Duration::from_millis(100));
            }
            {
                let mut salidas = last_seen_salidas.lock().await;
                salidas.retain(|_, t| now.duration_since(*t) < Duration::from_secs(5));
            }
            if let Ok(mut locks) = relay_locks.try_lock() {
                locks.retain(|_, lock| lock.load(Ordering::SeqCst));
            }
        }
        cleanup_counter += 1;
        
        tokio::select! {
            // Señal de reconexión desde el relay
            _ = reconnect_rx.recv() => {
                println!("🔄 Relay solicitó reconexión, reactivando inventario...");
                buffer.clear();
                
                if let Err(e) = forzar_reactivacion_completa(&mut stream).await {
                    println!("⚠️ Error reactivando: {}, reconectando completamente...", e);
                    match conectar_lector().await {
                        Ok(new_stream) => {
                            stream = new_stream;
                            println!("✅ Reconectado");
                        }
                        Err(recon_e) => {
                            println!("❌ Error reconexión: {}", recon_e);
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                } else {
                    println!("✅ Inventario reactivado exitosamente");
                }
                app.emit("rfid_estado", "conectado").unwrap();
            }
            
            // Lectura normal del socket
            result = tokio::time::timeout(Duration::from_millis(100), stream.read(&mut temp)) => {
                match result {
                    Ok(Ok(n)) if n > 0 => {
                        buffer.extend_from_slice(&temp[..n]);
                        while buffer.len() >= 8 {
                            if buffer[0] != 0xA5 || buffer[1] != 0x5A {
                                if let Some(pos) = buffer.windows(2).position(|w| w == [0xA5, 0x5A]) {
                                    buffer.drain(0..pos);
                                } else {
                                    buffer.clear();
                                    break;
                                }
                            }
                            let length = ((buffer[2] as usize) << 8) | buffer[3] as usize;
                            if buffer.len() < length { break; }
                            if length > 6 && buffer[4] == 0x83 {
                                let payload_end = length.saturating_sub(3);
                                if payload_end > 5 {
                                    let payload = buffer[5..payload_end].to_vec();
                                    if let Some((epc, antena)) = extraer_epc_universal(&payload) {
                                       // println!("🏷️ EPC: '{}' | Antena: {}", epc, antena);
                                        let now = Instant::now();
                                        
                                        let debe_procesar = {
                                            let mut global = last_seen_global.lock().await;
                                            if let Some(&last) = global.get(&epc) {
                                                if now.duration_since(last) < Duration::from_millis(50) {
                                                    false
                                                } else {
                                                    global.insert(epc.clone(), now);
                                                    true
                                                }
                                            } else {
                                                global.insert(epc.clone(), now);
                                                true
                                            }
                                        };
                                        
                                        if !debe_procesar {
                                            buffer.drain(0..length);
                                            continue;
                                        }
                                        
                                        total_lecturas += 1;
                                        stats_count += 1;
                                        if stats_timer.elapsed() >= Duration::from_secs(1) {
                                            println!("⚡ {} lecturas/seg (total: {})", stats_count, total_lecturas);
                                            app.emit("lecturas_por_segundo", stats_count).unwrap();
                                            stats_count = 0;
                                            stats_timer = Instant::now();
                                        }
                                        app.emit("tag_leido", &epc).unwrap();
                                        app.emit("contador_total", &total_lecturas).unwrap();
                                        
                                        if antena == 2 {
                                            // let procesar = {
                                            //     let mut salidas = last_seen_salidas.lock().await;
                                            //     if let Some(&last_salida) = salidas.get(&epc) {
                                            //         if now.duration_since(last_salida) < Duration::from_secs(5) {
                                            //             false
                                            //         } else {
                                            //             salidas.insert(epc.clone(), now);
                                            //             true
                                            //         }
                                            //     } else {
                                            //         salidas.insert(epc.clone(), now);
                                            //         true
                                            //     }
                                            // };
                                            
                                            let relay_locks_clone_inner = relay_locks.clone();
                                            let app_clone_inner = app.clone();
                                            let db_clone_inner = db_state.0.clone();
                                            let epc_clone = epc.clone();
                                            let reconnect_tx_clone = reconnect_tx.clone();
                                            let last_seen_salidas_clone = last_seen_salidas.clone();

                                            tokio::spawn(async move {
                                                println!("🚀 SPAWN iniciado para EPC: '{}'", epc_clone);
                                                // PASO 1: Adquirir el lock PRIMERO, antes de cualquier otra cosa
                                                let tag_lock = {
                                                    let mut locks = relay_locks_clone_inner.lock().await;
                                                    locks.entry(epc_clone.clone())
                                                        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                                                        .clone()
                                                };

                                                // PASO 2: Intentar "cerrar la puerta" atomicamente
                                                // Si ya esta cerrada (otro task llego primero), salir
                                                if tag_lock.compare_exchange(
                                                    false, true,
                                                    Ordering::SeqCst, Ordering::SeqCst
                                                ).is_err() {
                                                    println!("🔒 Lock ya tomado, saliendo para: '{}'", epc_clone);
                                                    return; // otro task ya tiene el lock → salir
                                                }

                                                println!("✅ Lock adquirido para: '{}'", epc_clone);

                                                // PASO 3: Ahora sí verificar el debounce de 5 segundos
                                                // Solo UNA task llega aqui
                                                let now = Instant::now();
                                                let debe_procesar_salida = {
                                                    let mut salidas = last_seen_salidas_clone.lock().await;
                                                    if let Some(&last) = salidas.get(&epc_clone) {
                                                        if now.duration_since(last) < Duration::from_secs(5) {
                                                            false // dentro del cooldown de 5s
                                                        } else {
                                                            salidas.insert(epc_clone.clone(), now);
                                                            true
                                                        }
                                                    } else {
                                                        salidas.insert(epc_clone.clone(), now);
                                                        true
                                                    }
                                                };
                                                println!("📋 debe_procesar_salida: {} para '{}'", debe_procesar_salida, epc_clone);

                                                if !debe_procesar_salida {
                                                    // Liberar el lock si no vamos a procesar
                                                    tag_lock.store(false, Ordering::SeqCst);
                                                    return;
                                                }

                                                // PASO 4: Logica de alerta (igual que antes)
                                                let es_alerta = {
                                                    let conn = db_clone_inner.lock().unwrap();
                                                    let desc = es_uso_interno(&conn, &epc_clone);
                                                    println!("🔍 es_uso_interno para '{}': {:?}", epc_clone, desc);
                                                    let alerta = desc.is_some();
                                                    let _ = conn.execute(
                                                        "INSERT INTO LecturasRFID_Salidas
                                                        (EPC, Antena, FechaLectura, Alerta)
                                                        VALUES (?1, 2, datetime('now'), ?2)",
                                                        params![&epc_clone, alerta as i32],
                                                    );
                                                    alerta
                                                };

                                                println!("🚨 es_alerta: {} para '{}'", es_alerta, epc_clone);


                                                if es_alerta {
                                                    app_clone_inner.emit("alerta_uso_interno", &epc_clone).unwrap();
                                                    //  relay_on().await;
                                                    //  tokio::time::sleep(Duration::from_secs(5)).await;
                                                    //  relay_off().await;
                                                    //  tokio::time::sleep(Duration::from_millis(100)).await;
                                                    //  let _ = reconnect_tx_clone.try_send(());
                                                }

                                                // Mantener el lock 10s y luego liberar
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                                tag_lock.store(false, Ordering::SeqCst);
                                            });
                                        } else {
                                            let _ = tx.try_send((epc, antena));
                                        }
                                    }
                                }
                            }
                            buffer.drain(0..length);
                        }
                    }
                    Ok(Ok(_)) => { tokio::task::yield_now().await; }
                    Ok(Err(e)) => {
                        println!("Error: {}, reconectando...", e);
                        app.emit("rfid_estado", "reconectando").unwrap();
                        
                        match conectar_lector().await {
                            Ok(new_stream) => {
                                stream = new_stream;
                                buffer.clear();
                                println!("✅ Reconectado al lector");
                                app.emit("rfid_estado", "conectado").unwrap();
                            }
                            Err(recon_e) => {
                                println!("❌ Error reconexión: {}", recon_e);
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }
    
    println!("🛑 Lectura detenida - Total: {} lecturas", total_lecturas);
    app.emit("rfid_estado", "detenido").unwrap();
    Ok(())
}

// ─── COMANDO DETENER LECTURA ──────────────────────────────────────────────────
#[tauri::command]
fn detener_lectura(rfid_state: State<'_, RfidState>) {
    *rfid_state.0.lock().unwrap() = false;
    println!("🛑 Señal de detener enviada");
}

// ─── COMANDO PULSAR RELAY MANUALMENTE DESDE JS ───────────────────────────────
#[tauri::command]
async fn activar_relay_manual(relay_state: State<'_, RelayState>) -> Result<(), String> {
    relay_state.tx.try_send(RelayCommand::Trigger)
        .map_err(|_| "Línea de control física del relay saturada".to_string())
}

// ─── ENTRY POINT GENERAL DE TAURI ────────────────────────────────────────────
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenv().ok();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let db_path = app
                .path()
                .app_data_dir()
                .expect("Error obteniendo app_data_dir")
                .join("app.db");

            println!("📂 Base de datos local SQLite en: {:?}", db_path);

            std::fs::create_dir_all(db_path.parent().unwrap())
                .expect("Error creando directorio");

            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            init_db(&conn);

            let db_state = DbState(Arc::new(Mutex::new(conn)));
            let db_clone = db_state.0.clone();
            let rfid_state = RfidState(Arc::new(Mutex::new(false)));

            app.manage(db_state);
            app.manage(rfid_state);
            
            // 1. Quitamos el guion bajo para usar la variable 'relay_rx'
            let (relay_tx, mut relay_rx) = tokio::sync::mpsc::channel::<RelayCommand>(30);
            app.manage(RelayState { tx: relay_tx });

            // 2. NUEVO: Creamos el hilo asíncrono que escucha y ejecuta los comandos del relay
            tauri::async_runtime::spawn(async move {
                println!("🔊 Escuchador del canal del Relay activado");
                while let Some(comando) = relay_rx.recv().await {
                    match comando {
                        RelayCommand::Trigger => {
                            println!("🔓 RELAY ON (Comando manual recibido)");
                            relay_on().await;
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            relay_off().await;
                            println!("🔒 RELAY OFF (Comando manual finalizado)");
                        }
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                sincronizar_rfid_pendiente(db_clone).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            saludar,
            login,
            sincronizar,
            sincronizar_rfid_manual,
            iniciar_lectura,
            detener_lectura,
            activar_relay_manual
        ])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}