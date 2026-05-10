use bcrypt::{hash, verify, DEFAULT_COST};
use rusqlite::{Connection, params};
use std::sync::Mutex;
use tauri::{Manager, State};
use tiberius::{AuthMethod, Client, Config};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

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
        let password_hash = hash("1234", DEFAULT_COST).expect("Error hasheando contraseña");
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at)
             VALUES (?1, ?2, datetime('now'))",
            params!["admin", password_hash],
        )
        .expect("Error insertando usuario admin");

        println!("✅ Usuario admin creado por defecto");
    }
}

// ─── COMANDO SALUDAR (prueba) ─────────────────────────────────────────────────
#[tauri::command]
fn saludar(nombre: &str) -> String {
    format!("Hola {}", nombre)
}

// ─── COMANDO LOGIN ────────────────────────────────────────────────────────────
#[tauri::command]
fn login(state: State<DbState>, user: String, pass: String) -> bool {
    let conn = state.0.lock().unwrap();
    // Buscar el hash del usuario en SQLite
    let result = conn.query_row(
        "SELECT password_hash FROM users WHERE username = ?1",
        params![user],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(password_hash) => verify(&pass, &password_hash).unwrap_or(false),
        
        Err(_) => false,
    }
}

// ─── COMANDO SINCRONIZAR ──────────────────────────────────────────────────────
#[tauri::command]
async fn sincronizar(state: State<'_, DbState>) -> Result<String, String> {
    // Configurar conexión a SQL Server
    let mut config = Config::new();
    config.host("0_Ciberelectrik.mssql.somee.com");
    config.port(1433);
    config.database("0_Ciberelectrik");
    config.authentication(AuthMethod::sql_server("jlujan_SQLLogin_1", "yyeftklvtf"));
    config.trust_cert();

    // Conectar a SQL Server
    let tcp = TcpStream::connect(config.get_addr())
        .await
        .map_err(|e| format!("Error conectando al servidor: {}", e))?;

    let mut client = Client::connect(config, tcp.compat_write())
        .await
        .map_err(|e| format!("Error autenticando en SQL Server: {}", e))?;

    // Traer usuarios de SQL Server
    let rows = client
        .query("SELECT id, username, password_hash FROM users", &[])
        .await
        .map_err(|e| format!("Error consultando usuarios: {}", e))?
        .into_first_result()
        .await
        .map_err(|e| format!("Error leyendo resultados: {}", e))?;

    // Actualizar SQLite local
    let conn = state.0.lock().unwrap();
    let mut sincronizados = 0;

    for row in &rows {
        //let id: i32 = row.get(0).unwrap_or(0);
        let username: &str = row.get(1).unwrap_or("");
        let password_hash: &str = row.get(2).unwrap_or("");

        conn.execute(
            "INSERT INTO users (username, password_hash, created_at)
            VALUES (?1, ?2, datetime('now'))
            ON CONFLICT(username) DO UPDATE SET
                password_hash = excluded.password_hash",
            params![username, password_hash],
        )
        .map_err(|e| format!("Error actualizando SQLite: {}", e))?;

        sincronizados += 1;
    }

    Ok(format!("✅ {} usuarios sincronizados correctamente", sincronizados))
}

// ─── ENTRY POINT ─────────────────────────────────────────────────────────────
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
             // Ruta de la BD: funciona igual en desktop, Android e iOS
            let db_path = app
                .path()
                .app_data_dir()
                .expect("Error obteniendo app_data_dir")
                .join("app.db");

            println!("📂 Base de datos en: {:?}", db_path);
                // Crear la carpeta si no existe
            std::fs::create_dir_all(db_path.parent().unwrap())
                .expect("Error creando directorio de la BD");
            // Abrir o crear la base de datos
            let conn = Connection::open(&db_path).expect("Error abriendo SQLite");
            // Inicializar tablas y usuario por defecto
            init_db(&conn);
            // Registrar el estado global
            app.manage(DbState(Mutex::new(conn)));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![saludar, login, sincronizar])
        .run(tauri::generate_context!())
        .expect("Error iniciando Tauri");
}