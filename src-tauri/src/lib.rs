#[tauri::command]
fn saludar(nombre: &str) -> String {
    format!("Hola {}", nombre)
}

#[tauri::command]
fn login(user: String, pass: String) -> bool {
    user == "admin" && pass == "1234"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![saludar, login])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}