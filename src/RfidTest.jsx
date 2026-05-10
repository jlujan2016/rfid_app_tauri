import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function RfidTest() {
  const [mensaje, setMensaje] = useState("");

  async function ejecutarRust() {
    const respuesta = await invoke("saludar", {
      nombre: "RFID"
    });

    setMensaje(respuesta);
  }

  return (
    <div style={{ padding: 20 }}>
      <h2>Test RFID</h2>

      <button onClick={ejecutarRust}>
        Ejecutar Rust
      </button>

      <p>{mensaje}</p>
    </div>
  );
}

export default RfidTest;