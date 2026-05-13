import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Sonido rápido sin delay
const playBeep = () => {
    try {
        const AudioContext = window.AudioContext || window.webkitAudioContext;
        const audioContext = new AudioContext();
        const oscillator = audioContext.createOscillator();
        const gainNode = audioContext.createGain();
        
        oscillator.connect(gainNode);
        gainNode.connect(audioContext.destination);
        
        oscillator.frequency.value = 2400;
        gainNode.gain.value = 0.15;
        
        oscillator.start();
        gainNode.gain.exponentialRampToValueAtTime(0.00001, audioContext.currentTime + 0.1);
        oscillator.stop(audioContext.currentTime + 0.1);
        
        audioContext.resume();
    } catch (e) {
        // Silencio si no hay audio
    }
};

function RfidTest() {
  const [leyendo, setLeyendo] = useState(false);
  const [estado, setEstado] = useState("desconectado");
  const [tags, setTags] = useState({});
  const [ultimoTag, setUltimoTag] = useState(null);
  const [lecturasPorSegundo, setLecturasPorSegundo] = useState(0);
  
  const contadorSegundo = useRef(0);
  const lastTime = useRef(Date.now());
  const updatePending = useRef(false);
  const pendingUpdates = useRef({});

  // 🔥 ACTUALIZACIÓN INMEDIATA (sin throttle)
  const actualizarTag = (epc) => {
    const ahora = Date.now();
    
    // Contar lecturas/segundo
    contadorSegundo.current++;
    if (ahora - lastTime.current >= 1000) {
      setLecturasPorSegundo(contadorSegundo.current);
      contadorSegundo.current = 0;
      lastTime.current = ahora;
    }
    
    // Actualización DIRECTA del estado
    setTags(prev => {
      const newCount = (prev[epc]?.count || 0) + 1;
      return {
        ...prev,
        [epc]: {
          epc: epc,
          count: newCount,
          lastSeen: new Date().toLocaleTimeString(),
        },
      };
    });
    
    setUltimoTag({ epc, timestamp: ahora });
    playBeep();
  };

  useEffect(() => {
    // Listener SIN ningún throttle
    const unlisten = listen("tag_leido", (event) => {
      actualizarTag(event.payload);
    });

    const unlistenEstado = listen("rfid_estado", (event) => {
      setEstado(event.payload);
      if (event.payload === "conectado") setLeyendo(true);
      if (event.payload === "detenido") setLeyendo(false);
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenEstado.then(fn => fn());
    };
  }, []);

  async function iniciarLectura() {
    setLeyendo(true);
    setTags({});
    setUltimoTag(null);
    contadorSegundo.current = 0;
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

  const tagList = Object.values(tags).sort((a, b) => a.epc.localeCompare(b.epc));
  const totalLecturas = tagList.reduce((sum, t) => sum + t.count, 0);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>📡 RFID - ULTRA RÁPIDO</h2>
        <div style={{ display: "flex", gap: 10 }}>
          {lecturasPorSegundo > 0 && (
            <span style={{ color: "#ffaa44" }}>⚡ {lecturasPorSegundo}/seg</span>
          )}
          <div style={{ width: 10, height: 10, borderRadius: "50%", background: estado === "conectado" ? "#6bff8e" : "#aaa" }} />
          <span>{estado}</span>
        </div>
      </div>

      {ultimoTag && (
        <div style={styles.ultimoTag}>
          🏷️ {ultimoTag.epc}
        </div>
      )}

      <div style={styles.row}>
        <button onClick={iniciarLectura} disabled={leyendo} style={styles.btnGreen}>
          {leyendo ? "LEYENDO..." : "▶ INICIAR"}
        </button>
        <button onClick={detenerLectura} disabled={!leyendo} style={styles.btnRed}>
          ⏹ DETENER
        </button>
      </div>

      <div style={styles.stats}>
        <div>📊 Total lecturas: <strong>{totalLecturas}</strong></div>
        <div>🏷️ Tags únicos: <strong>{tagList.length}</strong></div>
      </div>

      <div style={styles.table}>
        {tagList.slice(0, 20).map((tag, idx) => (
          <div key={tag.epc} style={styles.rowTable}>
            <span style={{ width: 40 }}>{idx + 1}</span>
            <span style={{ flex: 1, color: "#6bff8e" }}>{tag.epc}</span>
            <span style={{ width: 80, color: "#e74c3c", fontWeight: "bold" }}>{tag.count}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

const styles = {
  container: { padding: 20, background: "#0a0a0a", minHeight: "100vh", color: "white", fontFamily: "monospace" },
  header: { display: "flex", justifyContent: "space-between", marginBottom: 16 },
  title: { color: "#e74c3c", margin: 0 },
  ultimoTag: { background: "#1a1a1a", padding: 10, borderRadius: 4, marginBottom: 12, textAlign: "center" },
  row: { display: "flex", gap: 10, marginBottom: 12 },
  btnGreen: { flex: 1, padding: 12, background: "#e74c3c", border: "none", color: "white", fontWeight: "bold", cursor: "pointer" },
  btnRed: { flex: 1, padding: 12, background: "#c0392b", border: "none", color: "white", fontWeight: "bold", cursor: "pointer" },
  stats: { display: "flex", gap: 20, marginBottom: 12, padding: 10, background: "#111", borderRadius: 4 },
  table: { background: "#111", borderRadius: 4, maxHeight: 400, overflow: "auto" },
  rowTable: { display: "flex", padding: 8, borderBottom: "1px solid #222" },
};

export default RfidTest;