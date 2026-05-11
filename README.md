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


## Flujo de Aplicativo v2
Tag detectado
        ↓
Guarda en SQLite (instantáneo)
        ↓
Emite evento al frontend INMEDIATAMENTE ← aquí debe ir
        ↓
Intenta guardar en SQL Server en background (no bloquea)