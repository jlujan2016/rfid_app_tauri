import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

function RfidTest() {
  const [leyendo, setLeyendo] = useState(false);
  const [estado, setEstado] = useState("desconectado");
  const [tags, setTags] = useState([]);
  const [sincMensaje, setSincMensaje] = useState("");
  const [sincronizando, setSincronizando] = useState(false);
  const unlistenRef = useRef([]);

  useEffect(() => {
    // Escuchar tags leídos en tiempo real
    const unlistenTag = listen("tag_leido", (event) => {
      const epc = event.payload;
      const fecha = new Date().toLocaleTimeString();
      setTags((prev) => [{ epc, fecha }, ...prev]);
    });

    // Escuchar estado del lector
    const unlistenEstado = listen("rfid_estado", (event) => {
      setEstado(event.payload);
      if (event.payload === "detenido") {
        setLeyendo(false);
      }
    });

    unlistenRef.current = [unlistenTag, unlistenEstado];

    return () => {
      unlistenRef.current.forEach((fn) => fn.then((f) => f()));
    };
  }, []);

  async function iniciarLectura() {
    setLeyendo(true);
    setTags([]);
    try {
      await invoke("iniciar_lectura");
    } catch (error) {
      setEstado("error: " + error);
      setLeyendo(false);
    }
  }

  async function detenerLectura() {
    await invoke("detener_lectura");
    setLeyendo(false);
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

  const estadoColor = {
    conectado: "#6bff8e",
    detenido: "#aaa",
    desconectado: "#aaa",
  }[estado] || "#ff6b6b";

  return (
    <div style={styles.container}>

      {/* HEADER */}
      <div style={styles.header}>
        <h2 style={styles.title}>📡 Panel RFID</h2>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ ...styles.dot, background: estadoColor }} />
          <span style={{ color: estadoColor, fontSize: 12 }}>{estado}</span>
        </div>
      </div>

      {/* BOTONES LECTURA */}
      <div style={styles.row}>
        <button
          onClick={iniciarLectura}
          disabled={leyendo}
          style={{ ...styles.btn, background: leyendo ? "#555" : "red" }}
        >
          {leyendo ? "⏳ Leyendo..." : "▶ Iniciar lectura"}
        </button>

        <button
          onClick={detenerLectura}
          disabled={!leyendo}
          style={{ ...styles.btn, background: !leyendo ? "#555" : "#c0392b" }}
        >
          ⏹ Detener
        </button>
      </div>

      {/* CONTADOR */}
      {tags.length > 0 && (
        <p style={styles.counter}>
          {tags.length} tag{tags.length !== 1 ? "s" : ""} leído{tags.length !== 1 ? "s" : ""}
        </p>
      )}

      {/* LISTA DE TAGS */}
      <div style={styles.tagList}>
        {tags.length === 0 ? (
          <p style={styles.empty}>
            {leyendo ? "Esperando tags..." : "Sin lecturas aún"}
          </p>
        ) : (
          tags.map((tag, i) => (
            <div key={i} style={styles.tagItem}>
              <span style={styles.tagEpc}>📦 {tag.epc}</span>
              <span style={styles.tagFecha}>{tag.fecha}</span>
            </div>
          ))
        )}
      </div>

      {/* SEPARADOR */}
      <hr style={styles.hr} />

      {/* BOTÓN SINCRONIZAR */}
      <button
        onClick={sincronizar}
        disabled={sincronizando}
        style={{ ...styles.btn, background: sincronizando ? "#555" : "#c0392b" }}
      >
        {sincronizando ? "Sincronizando..." : "🔄 Sincronizar con servidor"}
      </button>

      {sincMensaje && (
        <p style={{
          ...styles.mensaje,
          color: sincMensaje.startsWith("❌") ? "#ff6b6b" : "#6bff8e",
        }}>
          {sincMensaje}
        </p>
      )}

    </div>
  );
}

const styles = {
  container: {
    padding: 20,
    fontFamily: "monospace",
    background: "#0a0a0a",
    minHeight: "100vh",
    color: "white",
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 16,
  },
  title: {
    color: "red",
    margin: 0,
    fontSize: 18,
  },
  dot: {
    width: 10,
    height: 10,
    borderRadius: "50%",
  },
  row: {
    display: "flex",
    gap: 10,
    marginBottom: 12,
  },
  btn: {
    flex: 1,
    padding: "10px 16px",
    border: "none",
    color: "white",
    cursor: "pointer",
    fontWeight: "bold",
    fontFamily: "monospace",
    fontSize: 13,
  },
  counter: {
    color: "#aaa",
    fontSize: 12,
    margin: "4px 0 8px",
  },
  tagList: {
    border: "1px solid #222",
    minHeight: 200,
    maxHeight: 350,
    overflowY: "auto",
    padding: 10,
    marginBottom: 16,
  },
  tagItem: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    padding: "6px 0",
    borderBottom: "1px solid #1a1a1a",
  },
  tagEpc: {
    color: "#6bff8e",
    fontSize: 13,
  },
  tagFecha: {
    color: "#555",
    fontSize: 11,
  },
  empty: {
    color: "#444",
    textAlign: "center",
    marginTop: 60,
  },
  hr: {
    borderColor: "#222",
    margin: "16px 0",
  },
  mensaje: {
    marginTop: 8,
    padding: 10,
    border: "1px solid #333",
    fontSize: 13,
  },
};

export default RfidTest;