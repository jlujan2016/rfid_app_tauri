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
  const [contadorBackend, setContadorBackend] = useState(0); // 🔥 Nuevo: contador desde backend
  
  const contadorSegundo = useRef(0);
  const lastTime = useRef(Date.now());

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
    // Listener para tags leídos
    const unlisten = listen("tag_leido", (event) => {
      actualizarTag(event.payload);
    });

    // Listener para estado del lector
    const unlistenEstado = listen("rfid_estado", (event) => {
      setEstado(event.payload);
      if (event.payload === "conectado") setLeyendo(true);
      if (event.payload === "detenido") setLeyendo(false);
    });

    // 🔥 NUEVO: Listener para contador total desde backend (opcional)
    const unlistenContador = listen("contador_total", (event) => {
      setContadorBackend(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenEstado.then(fn => fn());
      unlistenContador.then(fn => fn());
    };
  }, []);

  async function iniciarLectura() {
    setLeyendo(true);
    setTags({});
    setUltimoTag(null);
    setContadorBackend(0);
    contadorSegundo.current = 0;
    lastTime.current = Date.now();
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

  async function limpiarHistorial() {
    if (confirm("¿Borrar todas las lecturas acumuladas?")) {
      setTags({});
      setUltimoTag(null);
      setContadorBackend(0);
      contadorSegundo.current = 0;
    }
  }

  const tagList = Object.values(tags).sort((a, b) => b.count - a.count); // Ordenar por más lecturas
  const totalLecturas = tagList.reduce((sum, t) => sum + t.count, 0);
  const totalTags = tagList.length;

  return (
    <div style={styles.container}>
      {/* HEADER */}
      <div style={styles.header}>
        <h2 style={styles.title}>📡 RFID - ULTRA RÁPIDO</h2>
        <div style={{ display: "flex", gap: 15, alignItems: "center" }}>
          {lecturasPorSegundo > 0 && (
            <span style={{ 
              color: lecturasPorSegundo > 20 ? "#6bff8e" : "#ffaa44",
              fontWeight: "bold",
              fontSize: 12
            }}>
              ⚡ {lecturasPorSegundo}/seg
            </span>
          )}
          {contadorBackend > 0 && (
            <span style={{ color: "#888", fontSize: 11 }}>
              📊 BD: {contadorBackend}
            </span>
          )}
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <div style={{ 
              width: 10, 
              height: 10, 
              borderRadius: "50%", 
              background: estado === "conectado" ? "#6bff8e" : "#aaa",
              boxShadow: estado === "conectado" ? "0 0 5px #6bff8e" : "none"
            }} />
            <span style={{ fontSize: 12, color: "#888" }}>{estado}</span>
          </div>
        </div>
      </div>

      {/* ÚLTIMO TAG DETECTADO */}
      {ultimoTag && (
        <div style={styles.ultimoTag}>
          <span style={{ fontSize: 12, color: "#888" }}>🏷️ ÚLTIMO TAG</span>
          <strong style={{ color: "#ff6b6b", fontSize: 20, marginLeft: 10 }}>
            {ultimoTag.epc}
          </strong>
        </div>
      )}

      {/* BOTONES */}
      <div style={styles.row}>
        <button 
          onClick={iniciarLectura} 
          disabled={leyendo} 
          style={{ ...styles.btn, background: leyendo ? "#555" : "#e74c3c" }}
        >
          {leyendo ? "⏳ LEYENDO..." : "▶ INICIAR"}
        </button>
        <button 
          onClick={detenerLectura} 
          disabled={!leyendo} 
          style={{ ...styles.btn, background: !leyendo ? "#555" : "#c0392b" }}
        >
          ⏹ DETENER
        </button>
        <button 
          onClick={limpiarHistorial} 
          style={{ ...styles.btn, background: "#34495e" }}
        >
          🗑️ LIMPIAR
        </button>
      </div>

      {/* ESTADÍSTICAS */}
      <div style={styles.stats}>
        <div style={styles.statBox}>
          <span style={styles.statNumber}>{totalLecturas.toLocaleString()}</span>
          <span style={styles.statLabel}>LECTURAS</span>
        </div>
        <div style={styles.statBox}>
          <span style={styles.statNumber}>{totalTags}</span>
          <span style={styles.statLabel}>TAGS ÚNICOS</span>
        </div>
        <div style={styles.statBox}>
          <span style={styles.statNumber}>
            {totalTags > 0 ? (totalLecturas / totalTags).toFixed(1) : 0}
          </span>
          <span style={styles.statLabel}>PROMEDIO/TAG</span>
        </div>
      </div>

      {/* TABLA DE TAGS */}
      <div style={styles.tableContainer}>
        <div style={styles.tableHeader}>
          <span style={{ width: 50, textAlign: "center" }}>#</span>
          <span style={{ flex: 2 }}>EPC</span>
          <span style={{ width: 90, textAlign: "center" }}>COUNT</span>
          <span style={{ width: 110, textAlign: "center" }}>ÚLTIMA VEZ</span>
        </div>
        <div style={styles.tableBody}>
          {tagList.length === 0 ? (
            <p style={styles.empty}>
              {leyendo ? "🔄 Acerca un tag a la antena..." : "⚡ Presiona INICIAR"}
            </p>
          ) : (
            tagList.slice(0, 50).map((tag, idx) => (
              <div 
                key={tag.epc} 
                style={{
                  ...styles.tableRow,
                  background: ultimoTag?.epc === tag.epc ? "#2a1a1a" : "transparent",
                  transition: "background 0.1s",
                }}
              >
                <span style={{ width: 50, textAlign: "center", color: "#666" }}>{idx + 1}</span>
                <span style={{ flex: 2, color: "#6bff8e", fontSize: 12 }}>{tag.epc}</span>
                <span style={{ width: 90, textAlign: "center", color: "#ff6b6b", fontWeight: "bold", fontSize: 16 }}>
                  {tag.count}
                </span>
                <span style={{ width: 110, textAlign: "center", color: "#666", fontSize: 11 }}>
                  {tag.lastSeen || '—'}
                </span>
              </div>
            ))
          )}
        </div>
      </div>

      {/* INFO */}
      <div style={styles.infoBox}>
        <p style={{ margin: 0, fontSize: 11, color: "#555" }}>
          💡 Mientras el tag esté cerca de la antena (hasta 1 metro), 
          el <strong style={{ color: "#ffaa44" }}>COUNT</strong> se incrementa continuamente.
          <br />
          ⚡ Velocidad: hasta <strong>30+ lecturas/segundo</strong>
        </p>
      </div>
    </div>
  );
}

const styles = {
  container: { 
    padding: 20, 
    background: "#0a0a0a", 
    minHeight: "100vh", 
    color: "white", 
    fontFamily: "monospace" 
  },
  header: { 
    display: "flex", 
    justifyContent: "space-between", 
    alignItems: "center", 
    marginBottom: 16 
  },
  title: { color: "#e74c3c", margin: 0, fontSize: 18 },
  ultimoTag: { 
    background: "#1a1a1a", 
    padding: "12px", 
    borderRadius: 6, 
    marginBottom: 12, 
    textAlign: "center",
    border: "1px solid #2a2a2a"
  },
  row: { display: "flex", gap: 10, marginBottom: 16 },
  btn: { 
    flex: 1, 
    padding: "12px", 
    border: "none", 
    color: "white", 
    fontWeight: "bold", 
    cursor: "pointer",
    borderRadius: 6,
    fontFamily: "monospace",
    fontSize: 13,
    transition: "all 0.2s"
  },
  stats: { 
    display: "flex", 
    gap: 10, 
    marginBottom: 16 
  },
  statBox: { 
    flex: 1, 
    background: "#111", 
    padding: "10px", 
    borderRadius: 6, 
    textAlign: "center",
    border: "1px solid #222"
  },
  statNumber: { 
    fontSize: 24, 
    fontWeight: "bold", 
    color: "#e74c3c", 
    display: "block" 
  },
  statLabel: { 
    fontSize: 10, 
    color: "#555", 
    marginTop: 4,
    letterSpacing: "1px"
  },
  tableContainer: { 
    border: "1px solid #222", 
    borderRadius: 6, 
    overflow: "hidden", 
    marginBottom: 16 
  },
  tableHeader: { 
    display: "flex", 
    padding: "10px", 
    background: "#111", 
    borderBottom: "1px solid #222", 
    fontSize: 11, 
    color: "#888",
    textTransform: "uppercase"
  },
  tableBody: { 
    maxHeight: 400, 
    overflowY: "auto" 
  },
  tableRow: { 
    display: "flex", 
    alignItems: "center", 
    padding: "8px 10px", 
    borderBottom: "1px solid #111" 
  },
  empty: { 
    color: "#333", 
    textAlign: "center", 
    padding: "40px 0", 
    fontSize: 12 
  },
  infoBox: { 
    background: "#0a1a0a", 
    border: "1px solid #1a3a1a", 
    padding: 10, 
    borderRadius: 6,
    textAlign: "center"
  },
};

export default RfidTest;