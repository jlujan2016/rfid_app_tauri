import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function RfidTest() {
  const [mensaje, setMensaje] = useState("");
  const [sincMensaje, setSincMensaje] = useState("");
  const [sincronizando, setSincronizando] = useState(false);

  async function ejecutarRust() {
    const respuesta = await invoke("saludar", { nombre: "RFID" });
    setMensaje(respuesta);
  }

  async function sincronizar() {
    setSincronizando(true);
    setSincMensaje("Sincronizando...");
    try {
      const resultado = await invoke("sincronizar");
      setSincMensaje(resultado);
    } catch (error) {
      setSincMensaje("❌ Error: " + error);
    } finally {
      setSincronizando(false);
    }
  }

  return (
    <div style={{ padding: 20, fontFamily: "monospace", background: "#0a0a0a", minHeight: "100vh", color: "white" }}>
      <h2 style={{ color: "red" }}>Panel RFID</h2>

      {/* Botón de prueba Rust */}
      <button onClick={ejecutarRust} style={buttonStyle}>
        Ejecutar Rust
      </button>
      {mensaje && <p style={{ color: "#aaa", marginTop: 8 }}>{mensaje}</p>}

      {/* Separador */}
      <hr style={{ borderColor: "#333", margin: "20px 0" }} />

      {/* Botón sincronizar */}
      <button
        onClick={sincronizar}
        disabled={sincronizando}
        style={{ ...buttonStyle, background: sincronizando ? "#555" : "#c0392b" }}
      >
        {sincronizando ? "Sincronizando..." : "🔄 Sincronizar con servidor"}
      </button>

      {sincMensaje && (
        <p style={{
          marginTop: 10,
          padding: "10px",
          border: "1px solid #333",
          color: sincMensaje.startsWith("❌") ? "#ff6b6b" : "#6bff8e",
        }}>
          {sincMensaje}
        </p>
      )}
    </div>
  );
}

const buttonStyle = {
  padding: "10px 20px",
  background: "red",
  border: "none",
  color: "white",
  cursor: "pointer",
  fontWeight: "bold",
  fontFamily: "monospace",
  fontSize: "14px",
  marginTop: "10px",
};

export default RfidTest;