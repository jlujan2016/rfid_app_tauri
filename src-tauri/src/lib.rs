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
// DbState: contiene la conexión SQLite protegida por Mutex para acceso seguro entre threads
// RfidState: bandera booleana para controlar si el loop de lectura está activo
pub struct DbState(pub Mutex<Connection>);
pub struct RfidState(pub Mutex<bool>); // true = leyendo, false = detenido

// ─── HELPER MD5 ──────────────────────────────────────────────────────────────
// Genera un hash MD5 en formato hexadecimal lowercase
// Usado para hashear contraseñas al crear usuarios y al validar login
fn hash_md5(input: &str) -> String {
    format!("{:x}", md5::compute(input))
}

// ─── CONSTRUCTOR DE COMANDOS RFID ─────────────────────────────────────────────
// Construye el frame binario que entiende el lector UR4
// Estructura: [0xA5][0x5A][LEN_HI][LEN_LO][CMD][DATA...][CHECKSUM][0x0D][0x0A]
// El checksum es XOR de todos los bytes del header + data
fn build_command(command: u8, data: &[u8]) -> Vec<u8> {
    let length = (8 + data.len()) as u16;
    let mut checksum: u8 = ((length >> 8) as u8) ^ (length as u8) ^ command;
    for b in data {
        checksum ^= b;
    }
    let mut frame = Vec::new();
    frame.push(0xA5);                    // byte de inicio 1
    frame.push(0x5A);                    // byte de inicio 2
    frame.push((length >> 8) as u8);     // longitud high byte
    frame.push(length as u8);            // longitud low byte
    frame.push(command);                 // comando
    frame.extend_from_slice(data);       // datos del comando
    frame.push(checksum);                // checksum XOR
    frame.push(0x0D);                    // CR
    frame.push(0x0A);                    // LF
    frame
}

// ─── INICIALIZAR BASE DE DATOS ────────────────────────────────────────────────
// Crea las tablas si no existen y genera el usuario admin por defecto
// Se ejecuta una sola vez al arrancar la app
fn init_db(conn: &Connection) {
    conn.execute_batch(
        // Tabla users: idéntica a SQL Server para facilitar sincronización
        // Tabla LecturasRFID: agrega columna 'sincronizado' para offline-first
        //   sincronizado = 0 → pendiente de subir al servidor
        //   sincronizado = 1 → ya fue enviado al servidor
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
        println!("✅ Usuario admin creado con MD5");
    }
}

// ─── FETCH USUARIOS DEL SERVIDOR ─────────────────────────────────────────────
// Conecta a SQL Server y trae todos los usuarios
// Retorna None si no hay conexión (sin lanzar error)
// Usado por login (sincronización automática) y por el comando sincronizar
async fn fetch_users_from_server() -> Option<Vec<(String, String)>> {
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert(); // necesario para servidores sin certificado SSL válido

    // Si no hay conexión, retorna None silenciosamente
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
// Inserta o actualiza usuarios en SQLite local
// ON CONFLICT actualiza el hash si el usuario ya existe (cambio de contraseña)
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
// Guarda cada lectura RFID en SQLite con sincronizado=0 (pendiente)
// Siempre guarda todas las lecturas con fecha para historial completo
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
// Intenta insertar el EPC en SQL Server
// Si no hay conexión, el EPC ya quedó guardado en SQLite con sincronizado=0
// El WHERE NOT EXISTS evita duplicados en el servidor
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
            println!("⚠️ Sin conexión, EPC pendiente en SQLite: {}", epc);
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

    match client.execute(query, &[&epc.as_str()]).await {
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
// Comando de prueba para verificar comunicación Rust <-> React
#[tauri::command]
fn saludar(nombre: &str) -> String {
    format!("Hola {}", nombre)
}

// ─── COMANDO LOGIN ────────────────────────────────────────────────────────────
// Flujo offline-first:
// 1. Intenta sincronizar usuarios desde SQL Server (silencioso si falla)
// 2. Valida siempre contra SQLite local (funciona sin red)
// 3. Compara contraseña con hash MD5
#[tauri::command]
async fn login(state: State<'_, DbState>, user: String, pass: String) -> Result<bool, String> {
    // Paso 1: fetch async sin tener el Mutex (evita error Send)
    let users_from_server = fetch_users_from_server().await;

    // Paso 2: tomar el Mutex solo para operaciones sync
    let conn = state.0.lock().unwrap();

    if let Some(users) = users_from_server {
        let count = save_users_to_sqlite(&conn, users);
        println!("✅ Sincronización automática: {} usuarios", count);
    }

    // Paso 3: validar con MD5 contra SQLite
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
        Err(_) => Ok(false), // usuario no encontrado
    }
}

// ─── COMANDO SINCRONIZAR MANUAL ───────────────────────────────────────────────
// Fuerza sincronización de usuarios desde SQL Server
// Útil también para subir lecturas RFID pendientes en el futuro
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
// Loop principal de lectura RFID:
// 1. Conecta al lector por TCP (IP fija 192.168.1.180:8888)
// 2. Envía comandos de configuración al UR4
// 3. Lee frames binarios en loop
// 4. Decodifica EPCs del frame 0x83 (inventory response)
// 5. Aplica cache de 2 segundos para evitar lecturas duplicadas
// 6. ORDEN CRÍTICO: SQLite → emitir evento → SQL Server en background
//    Esto garantiza que el frontend se actualiza instantáneamente
#[tauri::command]
async fn iniciar_lectura(
    app: AppHandle,
    db_state: State<'_, DbState>,
    rfid_state: State<'_, RfidState>,
) -> Result<(), String> {
    // Marcar lectura como activa
    *rfid_state.0.lock().unwrap() = true;

    // Conectar al lector RFID por TCP
    let mut stream = TcpStream::connect("192.168.1.180:8888")
        .await
        .map_err(|e| format!("Error conectando al lector RFID: {}", e))?;

    println!("✅ Conectado al lector RFID");
    app.emit("rfid_estado", "conectado").unwrap();

    // Comando 0x60: configurar en modo lector
    stream
        .write_all(&build_command(0x60, &[0x01]))
        .await
        .map_err(|e| format!("Error enviando comando modo lector: {}", e))?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Comando 0x82: iniciar inventario continuo
    stream
        .write_all(&build_command(0x82, &[0x00, 0x00]))
        .await
        .map_err(|e| format!("Error iniciando inventario: {}", e))?;

    println!("📡 Inventario iniciado");

    let mut buffer: Vec<u8> = Vec::new();
    let mut temp = [0u8; 1024];
    // Cache para evitar mostrar el mismo tag más de una vez cada 2 segundos
    let mut cache: HashMap<String, Instant> = HashMap::new();

    loop {
        // Verificar si se solicitó detener antes de cada lectura
        if !*rfid_state.0.lock().unwrap() {
            println!("🛑 Lectura RFID detenida");
            app.emit("rfid_estado", "detenido").unwrap();
            break;
        }

        // Leer con timeout de 100ms para poder revisar el estado periódicamente
        let n = match tokio::time::timeout(
            Duration::from_millis(100),
            stream.read(&mut temp),
        )
        .await
        {
            Ok(Ok(n)) => n,
            Ok(Err(_)) => break,   // error de lectura, salir del loop
            Err(_) => continue,    // timeout, volver a revisar estado
        };

        if n == 0 {
            continue;
        }

        buffer.extend_from_slice(&temp[..n]);

        let mut i = 0;
        while i + 4 < buffer.len() {
            // Buscar inicio de frame: 0xA5 0x5A
            if buffer[i] != 0xA5 || buffer[i + 1] != 0x5A {
                i += 1;
                continue;
            }

            let length = ((buffer[i + 2] as usize) << 8) | buffer[i + 3] as usize;

            // Esperar hasta tener el frame completo
            if i + length > buffer.len() {
                break;
            }

            let frame = &buffer[i..i + length];

            // Frame 0x83 = respuesta de inventario (contiene EPCs)
            if frame.len() > 6 && frame[4] == 0x83 {
                let payload = &frame[5..frame.len().saturating_sub(2)];
                let mut found = String::new();

                // Buscar EPC válido en el payload usando ventana de 4 bytes
                // Heurística: EPCs del lector UR4 comienzan con 0041 o 0040
                for window in payload.windows(4) {
                    let candidate = hex::encode(window);
                    if candidate.starts_with("0041") || candidate.starts_with("0040") {
                        found = candidate;
                        break;
                    }
                }

                if !found.is_empty() {
                    let now = Instant::now();

                    // Filtro de duplicados: ignorar si el mismo tag fue leído hace menos de 2s
                    if let Some(last) = cache.get(&found) {
                        if now.duration_since(*last) < Duration::from_secs(2) {
                            i += length;
                            continue;
                        }
                    }
                    cache.insert(found.clone(), now);

                    println!("📦 TAG: {}", found);

                    // ─── ORDEN CRÍTICO PARA SINCRONÍA ────────────────────────
                    // 1. Guardar en SQLite primero (instantáneo, offline-first)
                    {
                        let conn = db_state.0.lock().unwrap();
                        guardar_epc_sqlite(&conn, &found);
                    }

                    // 2. Emitir al frontend INMEDIATAMENTE (sin esperar el server)
                    //    Esto garantiza que el tag aparece en pantalla al instante
                    app.emit("tag_leido", &found).unwrap();

                    // 3. Guardar en SQL Server en background (no bloquea el loop)
                    //    Si falla, el EPC queda en SQLite con sincronizado=0
                    tokio::spawn(guardar_epc_server(found.clone()));
                }
            }

            i += length;
        }

        // Limpiar bytes ya procesados del buffer
        buffer.drain(0..i);
    }

    Ok(())
}

// ─── COMANDO DETENER LECTURA RFID ─────────────────────────────────────────────
// Pone la bandera RfidState en false
// El loop en iniciar_lectura detecta el cambio en el siguiente ciclo (max 100ms)
#[tauri::command]
fn detener_lectura(rfid_state: State<'_, RfidState>) {
    *rfid_state.0.lock().unwrap() = false;
    println!("🛑 Señal de detener enviada");
}

// ─── ENTRY POINT ─────────────────────────────────────────────────────────────
// Punto de entrada de la app para desktop y mobile
// Inicializa la BD, registra los estados globales y los comandos disponibles
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Ruta de la BD adaptada automáticamente por plataforma:
            // Desktop  → AppData\Roaming\com.lujan.rfid-app-tauri\app.db
            // Android  → /data/data/com.lujan.rfid_app_tauri/app.db
            let db_path = app
                .path()
                .app_data_dir()
                .expect("Error obteniendo app_data_dir")
                .join("app.db");

            println!("📂 Base de datos en: {:?}", db_path);

            // Crear directorio si no existe
            std::fs::create_dir_all(db_path.parent().unwrap())
                .expect("Error creando directorio de la BD");

            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            init_db(&conn);

            // Registrar estados globales accesibles desde cualquier comando
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