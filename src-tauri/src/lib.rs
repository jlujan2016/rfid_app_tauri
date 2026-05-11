use bcrypt::{hash, verify, DEFAULT_COST};
use rusqlite::{Connection, params};
use std::sync::Mutex;
use tauri::{Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use md5;

// ─── ESTADO GLOBAL ───────────────────────────────────────────────────────────
pub struct DbState(pub Mutex<Connection>);

// ─── INICIALIZAR BASE DE DATOS ────────────────────────────────────────────────
fn init_db(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY,
            username      TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at    TEXT NOT NULL
        );",
    )
    .expect("Error creando tabla users");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE username = 'admin'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if count == 0 {
        // reemplaza la línea del hash en init_db
        let password_hash = format!("{:x}", md5::compute("1234"));
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at)
             VALUES (?1, ?2, datetime('now'))",
            params!["admin", password_hash],
        )
        .expect("Error insertando usuario admin");
        println!("✅ Usuario admin creado por defecto");
    }
}

// ─── SINCRONIZACIÓN INTERNA (retorna usuarios) ────────────────────────────────
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

    let result = client
        .query("SELECT username, password_hash FROM users", &[])
        .await;

    let stream = match result {
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

// ─── COMANDO SALUDAR ──────────────────────────────────────────────────────────
#[tauri::command]
fn saludar(nombre: &str) -> String {
    format!("Hola {}", nombre)
}

// ─── COMANDO LOGIN ────────────────────────────────────────────────────────────
#[tauri::command]
async fn login(state: State<'_, DbState>, user: String, pass: String) -> Result<bool, String> {
    // 1. Fetch users from server (async, sin tener el Mutex)
    let users_from_server = fetch_users_from_server().await;

    // 2. Guardar en SQLite y validar (con Mutex, sync)
    let conn = state.0.lock().unwrap();

    if let Some(users) = users_from_server {
        let count = save_users_to_sqlite(&conn, users);
        println!("✅ Sincronización automática: {} usuarios", count);
    }

    // 3. Validar contra SQLite
    let result = conn.query_row(
        "SELECT password_hash FROM users WHERE username = ?1",
        params![user],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(password_hash) => {
            let input_hash = format!("{:x}", md5::compute(&pass));
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![saludar, login, sincronizar])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}