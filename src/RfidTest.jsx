import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Sonido rápido optimizado
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
    const [contadorBackend, setContadorBackend] = useState(0);
    const [sincronizando, setSincronizando] = useState(false);
    const [mensajeSync, setMensajeSync] = useState("");
    const [alertas, setAlertas] = useState([]);
    const [panelAbierto, setPanelAbierto] = useState(false);
    
    const contadorSegundo = useRef(0);
    const lastTime = useRef(Date.now());
    const lastBeepTime = useRef({});
    const pendientes = useRef([]);
    // Beep con throttle (max 1 cada 300ms por tag)
    const playOptimizedBeep = (epc) => {
        const ahora = Date.now();
        const lastBeep = lastBeepTime.current[epc] || 0;
        if (ahora - lastBeep > 300) {
            lastBeepTime.current[epc] = ahora;
            playBeep();
        }
    };
    
    // Escuchar eventos del backend
    useEffect(() => {
        let unlistenTag, unlistenEstado, unlistenLecturas, unlistenContador, unlistenAlerta;
        
        const setupListeners = async () => {
            unlistenTag = await listen("tag_leido", (event) => {
                const epc = event.payload;
                const ahora = Date.now();

                 // solo acumula, no actualiza estado directamente
                pendientes.current.push(event.payload);
                
                // Contador para métricas
                contadorSegundo.current++;
                if (ahora - lastTime.current >= 1000) {
                    setLecturasPorSegundo(contadorSegundo.current);
                    contadorSegundo.current = 0;
                    lastTime.current = ahora;
                }
                
                // Actualización eficiente de estado
                setTags(prev => {
                    const existing = prev[epc];
                    const newCount = (existing?.count || 0) + 1;
                    
                    return {
                        ...prev,
                        [epc]: {
                            epc: epc,
                            count: newCount,
                            lastSeen: new Date().toLocaleTimeString()
                        }
                    };
                });
                
                setUltimoTag({ epc, timestamp: ahora });
                playOptimizedBeep(epc);
            });
            
            unlistenEstado = await listen("rfid_estado", (event) => {
                setEstado(event.payload);
                if (event.payload === "detenido") {
                    setLeyendo(false);
                }
            });
            
            unlistenLecturas = await listen("lecturas_por_segundo", (event) => {
                setLecturasPorSegundo(event.payload);
            });
            
            unlistenContador = await listen("contador_total", (event) => {
                setContadorBackend(event.payload);
            });
            
            unlistenAlerta = await listen("alerta_uso_interno", (event) => {
                setAlertas(prev => [{
                    id: Date.now(),
                    epc: event.payload,
                    hora: new Date().toLocaleTimeString()
                }, ...prev].slice(0, 50));
            });
        };
        
        // Agrega este intervalo dentro del useEffect:
const intervalo = setInterval(() => {
    if (pendientes.current.length === 0) return;
    
    const lote = [...pendientes.current];
    pendientes.current = [];

    // un solo beep por lote
    playBeep();

    // actualizar ultimo tag
    setUltimoTag({ epc: lote[lote.length - 1], timestamp: Date.now() });

    // actualizar tabla de una sola vez
    setTags(prev => {
        const nuevo = { ...prev };
        for (const epc of lote) {
            const count = (nuevo[epc]?.count || 0) + 1;
            nuevo[epc] = {
                epc,
                count,
                lastSeen: new Date().toLocaleTimeString(),
            };
        }
        return nuevo;
    });
}, 100); // actualiza pantalla cada 100ms

        setupListeners();
        
        return () => {
            clearInterval(intervalo);
            if (unlistenTag) unlistenTag.then(fn => fn());
            if (unlistenEstado) unlistenEstado.then(fn => fn());
            if (unlistenLecturas) unlistenLecturas.then(fn => fn());
            if (unlistenContador) unlistenContador.then(fn => fn());
            if (unlistenAlerta) unlistenAlerta.then(fn => fn());
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
    
    async function sincronizar() {
        setSincronizando(true);
        setMensajeSync("");
        try {
            const resultado = await invoke("sincronizar");
            setMensajeSync(resultado);
        } catch (error) {
            setMensajeSync("❌ Error: " + error);
        } finally {
            setSincronizando(false);
        }
    }
    
    const tagList = Object.values(tags).sort((a, b) => b.count - a.count);
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
                            📊 Total: {contadorBackend}
                        </span>
                    )}
                    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <div style={{ 
                            width: 10, 
                            height: 10, 
                            borderRadius: "50%", 
                            background: estado === "conectado" ? "#6bff8e" : estado === "reconectando" ? "#ffaa44" : "#aaa",
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
                <button 
                    onClick={sincronizar}
                    disabled={sincronizando || leyendo}
                    style={{ ...styles.btn, background: sincronizando ? "#555" : "#2980b9" }}
                >
                    {sincronizando ? "⏳ SINCRONIZANDO..." : "🔄 SINCRONIZAR"}
                </button>
                <button
                    onClick={() => setPanelAbierto(!panelAbierto)}
                    style={{
                        ...styles.btn,
                        background: alertas.length > 0 ? "#c0392b" : "#34495e",
                        position: "relative"
                    }}
                >
                    🚨 ALERTAS
                    {alertas.length > 0 && (
                        <span style={{
                            position: "absolute",
                            top: -6,
                            right: -6,
                            background: "#ff0000",
                            color: "white",
                            borderRadius: "50%",
                            width: 18,
                            height: 18,
                            fontSize: 11,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                            fontWeight: "bold"
                        }}>
                            {alertas.length}
                        </span>
                    )}
                </button>
            </div>
            
            {/* MENSAJE SINCRONIZACION */}
            {mensajeSync && (
                <div style={{ 
                    marginTop: 8, 
                    padding: "8px 12px", 
                    background: "#111", 
                    border: "1px solid #222",
                    borderRadius: 6,
                    fontSize: 11,
                    color: "#6bff8e",
                    textAlign: "center"
                }}>
                    {mensajeSync}
                </div>
            )}
            
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
                    ⚡ Velocidad optimizada: <strong style={{ color: "#6bff8e" }}>20-30+ lecturas/segundo</strong>
                    <br />
                    🔄 Filtro anti-duplicados: 50ms | Reconexión automática
                </p>
            </div>
            
            {/* PANEL LATERAL DE ALERTAS */}
            {panelAbierto && (
                <div style={{
                    position: "fixed",
                    top: 0,
                    right: 0,
                    width: 320,
                    height: "100vh",
                    background: "#0f0f0f",
                    borderLeft: "2px solid #c0392b",
                    zIndex: 1000,
                    display: "flex",
                    flexDirection: "column",
                    padding: 16,
                    overflowY: "auto"
                }}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
                        <h3 style={{ color: "#c0392b", margin: 0, fontSize: 14 }}>
                            🚨 ALERTAS USO INTERNO ({alertas.length})
                        </h3>
                        <button
                            onClick={() => setPanelAbierto(false)}
                            style={{ background: "none", border: "none", color: "#888", cursor: "pointer", fontSize: 18 }}
                        >
                            ✕
                        </button>
                    </div>
                    
                    {alertas.length === 0 ? (
                        <p style={{ color: "#444", fontSize: 12, textAlign: "center" }}>
                            Sin alertas registradas
                        </p>
                    ) : (
                        alertas.map(alerta => (
                            <div key={alerta.id} style={{
                                background: "#1a0000",
                                border: "1px solid #c0392b",
                                borderRadius: 6,
                                padding: 12,
                                marginBottom: 10
                            }}>
                                <div style={{ color: "#ff6b6b", fontWeight: "bold", fontSize: 13 }}>
                                    🏷️ {alerta.epc}
                                </div>
                                <div style={{ color: "#666", fontSize: 11, marginTop: 4 }}>
                                    🕐 {alerta.hora}
                                </div>
                                <div style={{ color: "#ff4444", fontSize: 11, marginTop: 4 }}>
                                    ⚠️ Equipo uso interno — salida no autorizada
                                </div>
                            </div>
                        ))
                    )}
                    
                    {alertas.length > 0 && (
                        <button
                            onClick={() => setAlertas([])}
                            style={{
                                marginTop: "auto",
                                padding: 10,
                                background: "#1a1a1a",
                                border: "1px solid #333",
                                color: "#888",
                                borderRadius: 6,
                                cursor: "pointer",
                                fontSize: 12
                            }}
                        >
                            🗑️ Limpiar historial
                        </button>
                    )}
                </div>
            )}
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
    row: { display: "flex", gap: 10, marginBottom: 16, flexWrap: "wrap" },
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
        transition: "all 0.2s",
        minWidth: "100px"
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