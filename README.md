# RFID App - Tauri + React

Sistema de lectura RFID con interfaz 3D industrial.

## Tecnologías
- React
- Tauri
- Rust
- Three.js

## Ejecutar
npm install
### Windows
npm run tauri dev
### Android
cargo tauri android dev

## Para generar ejecutable
### Windows
npm install
npm run tauri build
### Android
cargo tauri android build


## Flujo de Aplicativo v1
Tag detectado por lector RFID
        ↓
Rust procesa el frame (decodifica EPC)
        ↓
Guarda en SQLite (sync)
        ↓
Intenta guardar en SQL Server (async - AQUÍ ESTÁ EL RETRASO)
        ↓
Emite evento al frontend "tag_leido"
        ↓
React recibe el evento y muestra el tag