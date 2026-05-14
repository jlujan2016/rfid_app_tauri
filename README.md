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

# 📱 RFID App Tauri - Guía de Firmado para Android

> Guía completa para generar un APK standalone firmado para Android con Tauri

---

## 📋 Prerrequisitos

- Tauri CLI instalado
- Android Studio con SDK configurado
- Java JDK 17+
- Dispositivo Android con depuración USB (opcional)

---

## 📁 Estructura del Proyecto
rfid-app-tauri/

│

├── rfid-app.keystore ← Keystore (en la raíz)

│

├── src-tauri/

│ ├── gen/

│ │ └── android/

│ │ ├── keystore.properties ← Configuración de firma

│ │ │

│ │ └── app/

│ │ └── build.gradle.kts ← Configuración Gradle (modificada)

│ │

│ └── tauri.conf.json

│

└── README.md


---

## 🔧 Configuración Inicial (SOLO UNA VEZ)

### 1. Generar el Keystore

Ejecuta en la **raíz del proyecto**:

```bash
keytool -genkey -v -keystore rfid-app.keystore -alias rfid -keyalg RSA -keysize 2048 -validity 10000
```
```bash
keytool -genkey -v -keystore rfid-app.keystore -alias rfid -keyalg RSA -keysize 2048 -validity 10000 -storepass 123456 -keypass 123456 -dname "CN=RFID App, OU=Development, O=Lujan, L=City, S=State, C=US"
```


**Datos generados:**

| Parámetro | Valor |
|-----------|-------|
| Archivo | `rfid-app.keystore` |
| Contraseña | `123456` |
| Alias | `rfid` |
| Validez | 10000 días (~27 años) |

> ⚠️ **IMPORTANTE:** Guarda este archivo en un lugar seguro. **NUNCA** lo subas a GitHub.


### 2. Crear archivo keystore.properties
Crea un archivo llamado `keystore.properties` en la ruta `src-tauri/gen/android/` con el siguiente contenido. Este archivo le indicará a Gradle dónde encontrar tu keystore y las credenciales para usarlo.

**Ruta:** `src-tauri/gen/android/keystore.properties`

**Contenido del archivo:**

storeFile=../../../../rfid-app.keystore
storePassword=123456
keyAlias=rfid
keyPassword=123456

**Explicación de cada línea:**

| Línea | Significado |
|-------|-------------|
| `storeFile=../../../../rfid-app.keystore` | Ruta relativa al keystore desde la carpeta android |
| `storePassword=123456` | Contraseña del keystore |
| `keyAlias=rfid` | Alias de la clave (el mismo que usaste al generar) |
| `keyPassword=123456` | Contraseña de la clave |

**Nota:** La ruta `../../../../rfid-app.keystore` sube 4 niveles desde `src-tauri/gen/android/` hasta llegar a la raíz del proyecto donde se encuentra el keystore.

### 3. Modificar build.gradle.kts
Ahora debes modificar el archivo de configuración de Gradle de la aplicación para que utilice la configuración de firmado que acabas de crear.

**Ruta:** `src-tauri/gen/android/app/build.gradle.kts`

Paso 3.1: Añadir el import

Al inicio del archivo `build.gradle.kts`, añade las siguientes líneas de importación. Estas líneas son necesarias para poder trabajar con archivos y propiedades dentro del script de Gradle:

**Explicación de los imports:**

| Import | Propósito |
|--------|-----------|
| `import java.io.File` | Permite manejar archivos del sistema, como el keystore |
| `import java.util.Properties` | Permite leer archivos de propiedades como `keystore.properties` |

**Ubicación exacta donde añadirlo:**

El archivo `build.gradle.kts` debe comenzar de esta manera:

import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

// ... resto del archivo ...

Paso 3.2: Añadir la configuración de firmado

Dentro del bloque `android { }`, después de `defaultConfig { }` y antes de `buildTypes { }`, añade la siguiente configuración de firmado:

android {
    // ... configuración existente ...
    
    defaultConfig {
        // ... configuración existente ...
    }
    
    // ⬇️⬇️⬇️ AÑADE ESTO ⬇️⬇️⬇️
    signingConfigs {
        create("release") {
            val propertiesFile = File(rootProject.projectDir, "../keystore.properties")
            if (propertiesFile.exists()) {
                val props = java.util.Properties()
                props.load(propertiesFile.inputStream())
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            }
        }
    }
    // ⬆️⬆️⬆️ HASTA AQUÍ ⬆️⬆️⬆️
    
    buildTypes {
        // ... configuración existente ...
    }
}


**Explicación del código:**

| Línea | Propósito |
|-------|-----------|
| `signingConfigs { create("release") { ... } }` | Crea una configuración de firmado llamada "release" |
| `File(rootProject.projectDir, "../keystore.properties")` | Busca el archivo `keystore.properties` en la carpeta correcta |
| `props.load(propertiesFile.inputStream())` | Carga las propiedades del archivo |
| `storeFile = file(props.getProperty("storeFile"))` | Lee la ruta del keystore |
| `storePassword = props.getProperty("storePassword")` | Lee la contraseña del keystore |
| `keyAlias = props.getProperty("keyAlias")` | Lee el alias de la clave |
| `keyPassword = props.getProperty("keyPassword")` | Lee la contraseña de la clave |

Paso 3.3: Modificar el bloque release

Dentro de `buildTypes { }`, busca la sección `getByName("release") { }` y añade la línea que asigna la configuración de firmado: 


        buildTypes {
        getByName("debug") {
        // ... se queda igual ...
        }
    
        getByName("release") {
             signingConfig = signingConfigs.getByName("release")  // ⬅️ AÑADE ESTA LÍNEA
             isMinifyEnabled = true
             proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                        .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                .toList().toTypedArray()
                )
        }
    }

**Ejemplo completo de cómo debe quedar la sección `buildTypes`:**

        buildTypes {
                getByName("debug") {
                 manifestPlaceholders["usesCleartextTraffic"] = "true"
                isDebuggable = true
                isJniDebuggable = true
                isMinifyEnabled = false
                packaging {
                        jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                        jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                        jniLibs.keepDebugSymbols.add("*/x86/*.so")
                        jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
        }
    }
    
    getByName("release") {
        signingConfig = signingConfigs.getByName("release")  // ⬅️ LÍNEA AÑADIDA
        isMinifyEnabled = true
        proguardFiles(
            *fileTree(".") { include("**/*.pro") }
                .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                .toList().toTypedArray()
        )
        }
        }


**Verificación final:**

Después de completar todos los pasos, tu archivo `build.gradle.kts` debería tener:

1. ✅ Los imports al inicio (`import java.io.File` y `import java.util.Properties`)
2. ✅ La sección `signingConfigs` dentro de `android { }`
3. ✅ La línea `signingConfig = signingConfigs.getByName("release")` dentro de `buildTypes.release`


🚀 Compilación e Instalación

Método Rápido (Todo en uno)

Ejecuta los siguientes comandos para compilar, firmar automáticamente (si la configuración está correcta) e instalar la app en tu dispositivo:

# 1. Compilar
cargo tauri android build --apk

# 2. Ir al APK
cd src-tauri/gen/android/app/build/outputs/apk/universal/release

# 3. Instalar
adb install app-universal-release.apk


Método Detallado (Paso a Paso)

Paso 1: Compilar el APK
Compila el proyecto para generar el APK release:

cargo tauri android build --apk

**Resultado esperado:**

Finished 1 APK at:
    src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk


Paso 2: Firmar Manualmente (si es necesario)

Si la configuración automática no funcionó y el APK se generó como `unsigned`, fírmalo manualmente con el siguiente comando:

cd src-tauri/gen/android/app/build/outputs/apk/universal/release

apksigner sign --ks ../../../../../../rfid-app.keystore --ks-key-alias rfid --ks-pass pass:123456 --key-pass pass:123456 --out app-universal-release-signed.apk app-universal-release-unsigned.apk

Paso 3: Verificar la Firma
Verifica que el APK esté correctamente firmado:

apksigner verify --verbose app-universal-release-signed.apk

**Salida correcta:**

Verifies
Verified using v1 scheme (JAR signing): false
Verified using v2 scheme (APK Signature Scheme v2): true
Verified using v3 scheme (APK Signature Scheme v3): true
Number of signers: 1


Paso 4: Instalar en el Dispositivo

Instala el APK en tu dispositivo Android con depuración USB activada:

# Instalar
adb install app-universal-release-signed.apk

# Si ya existe versión anterior, desinstalar primero
adb uninstall com.lujan.rfid_app_tauri
adb install app-universal-release-signed.apk

📂 Ubicación de los Archivos Generados
| Tipo | Ruta |
|------|------|
| **APK Release (unsigned)** | `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk` |
| **APK Release (firmado)** | `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-signed.apk` |
| **APK Debug** | `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |
| **App Bundle (AAB)** | `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/` |

🛠️ Comandos Útiles
**Información del Keystore:**

keytool -list -keystore rfid-app.keystore -storepass 123456

**Limpiar Build:**
cargo tauri android clean

**Instalar Versión Debug (sin firma):**
cd src-tauri/gen/android/app/build/outputs/apk/universal/debug
adb install app-universal-debug.apk

**Generar App Bundle para Google Play:**
cargo tauri android build

🐛 Solución de Problemas
| Error | Causa | Solución |
|-------|-------|----------|
| `INSTALL_PARSE_FAILED_NO_CERTIFICATES` | APK sin firmar | Firmar manualmente el APK |
| `INSTALL_FAILED_NO_MATCHING_ABIS` | Arquitectura incorrecta | Usar APK `universal` en lugar de `x86_64` |
| `FileNotFoundException: .keystore` | Ruta incorrecta | Verificar ruta en `keystore.properties` |
| `Cannot recover key` | Alias o contraseña incorrectos | Verificar alias con `keytool -list` |
| `Keystore was tampered with` | Contraseña incorrecta | Verificar contraseña del keystore |
| `INSTALL_FAILED_UPDATE_INCOMPATIBLE` | Versión anterior incompatible | `adb uninstall com.lujan.rfid_app_tauri` |

📝 Notas Importantes
- **Versión Debug:** No necesita firma, ideal para pruebas rápidas
- **Versión Release:** Debe estar firmada para instalación standalone
- **Seguridad:** Nunca subas el archivo `.keystore` a GitHub
- **Google Play:** Usa el AAB (App Bundle) para publicar en la tienda
- **Automatización:** Con la configuración correcta, los APK se firman automáticamente

🎯 Flujo Rápido para Futuras Compilaciones
Si ya realizaste la configuración inicial, solo necesitas:
# Compilar
cargo tauri android build --apk

# Instalar
cd src-tauri/gen/android/app/build/outputs/apk/universal/release
adb install app-universal-release.apk

✅ Verificación Rápida
# Verificar que el keystore existe
dir rfid-app.keystore

# Verificar el alias
keytool -list -keystore rfid-app.keystore -storepass 123456

# Verificar que el APK está firmado
apksigner verify --verbose app-universal-release-signed.apk

## Flujo de Aplicativo v2
Tag detectado
        ↓
Guarda en SQLite (instantáneo)
        ↓
Emite evento al frontend INMEDIATAMENTE ← aquí debe ir
        ↓
Intenta guardar en SQL Server en background (no bloquea)