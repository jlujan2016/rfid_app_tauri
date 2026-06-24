import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ─── AUDIO ────────────────────────────────────────────────────────────────────
const playBeep = () => {
    try {
        const Ctx = window.AudioContext || window.webkitAudioContext;
        const ctx = new Ctx();
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.frequency.value = 2400;
        gain.gain.value = 0.12;
        osc.start();
        gain.gain.exponentialRampToValueAtTime(0.00001, ctx.currentTime + 0.08);
        osc.stop(ctx.currentTime + 0.08);
        ctx.resume();
    } catch (_) {}
};

// ─── CONSTANTES ───────────────────────────────────────────────────────────────
const ANTENAS = [
    { id: 1, label: "Antena 1", desc: "Entrada principal" },
    { id: 2, label: "Antena 2", desc: "Salida / control" },
    { id: 3, label: "Antena 3", desc: "Ambiente secundario" },
];

const BADGE = {
    "Uso Interno": { bg: "#3d1a1a", color: "#ff6b6b", text: "Uso interno" },
    "Préstamo":    { bg: "#1a2d1a", color: "#6bff8e", text: "Préstamo" },
    default:       { bg: "#1a1a2d", color: "#6b9fff", text: "General" },
};

function tipoBadge(tipo) {
    return BADGE[tipo] || BADGE.default;
}

// ─── COMPONENTE PRINCIPAL ─────────────────────────────────────────────────────
export default function RfidTest() {
    // Estado de lectura
    const [leyendo, setLeyendo] = useState(false);
    const [estado, setEstado] = useState("desconectado");
    const [tags, setTags] = useState({});           // { epc: { epc, count, lastSeen, antena, descripcion, categoria, tipo } }
    const [ultimoTag, setUltimoTag] = useState(null);
    const [lps, setLps] = useState(0);
    const [contadorBackend, setContadorBackend] = useState(0);

    // Catálogo de equipos { epc → { descripcion, categoria, tipo, estado } }
    const [catalogo, setCatalogo] = useState({});
    const [categorias, setCategorias] = useState([]);        // lista única de categorías
    const [categoriasFiltro, setCategoriasFiltro] = useState(new Set()); // vacío = todas

    // Antenas activas (filtro de visualización — el backend emite todo)
    const [antenasFiltro, setAntenasFiltro] = useState(new Set([1, 2, 3]));

    // UI
    const [sincronizando, setSincronizando] = useState(false);
    const [mensajeSync, setMensajeSync] = useState("");
    const [alertas, setAlertas] = useState([]);
    const [panelAbierto, setPanelAbierto] = useState(false);
    const [busqueda, setBusqueda] = useState("");

    // Refs de rendimiento
    const contadorSeg = useRef(0);
    const lastTime = useRef(Date.now());
    const lastBeepTime = useRef({});
    const pendientes = useRef([]);
    const ultimaAlerta = useRef({});
    const catalogoRef = useRef({});   // copia síncrona para usar en listeners

    // ── Cargar catálogo al montar ─────────────────────────────────────────────
    useEffect(() => {
        cargarCatalogo();
    }, []);

    async function cargarCatalogo() {
        try {
            const equipos = await invoke("obtener_equipos");
            const map = {};
            const cats = new Set();
            for (const eq of equipos) {
                map[eq.epc] = eq; // eq ya trae: epc, descripcion, categoria, tipo, estado + marca, modelo
                if (eq.categoria) cats.add(eq.categoria);
            }
            setCatalogo(map);
            catalogoRef.current = map;
            setCategorias([...cats].sort());
        } catch (e) {
            console.error("Error cargando catálogo:", e);
        }
    }

    // ── Listeners de eventos ──────────────────────────────────────────────────
    useEffect(() => {
        let unTag, unEstado, unLps, unContador, unAlerta;

        const setup = async () => {
            unTag = await listen("tag_leido_detalle", (event) => {
                // El backend emite { epc, antena }
                const { epc, antena } = event.payload;
                pendientes.current.push({ epc, antena });
                contadorSeg.current++;
                const ahora = Date.now();
                if (ahora - lastTime.current >= 1000) {
                    setLps(contadorSeg.current);
                    contadorSeg.current = 0;
                    lastTime.current = ahora;
                }
            });

            // Fallback: si el backend sigue emitiendo "tag_leido" (solo string)
            // lo capturamos igual pero sin antena
            await listen("tag_leido", (event) => {
                pendientes.current.push({ epc: event.payload, antena: null });
                contadorSeg.current++;
            });

            unEstado = await listen("rfid_estado", (event) => {
                setEstado(event.payload);
                if (event.payload === "detenido") setLeyendo(false);
            });

            unLps = await listen("lecturas_por_segundo", (event) => {
                setLps(event.payload);
            });

            unContador = await listen("contador_total", (event) => {
                setContadorBackend(event.payload);
            });

            unAlerta = await listen("alerta_uso_interno", (event) => {
                const epc = event.payload;
                const ahora = Date.now();
                if (ahora - (ultimaAlerta.current[epc] || 0) < 30_000) return;
                ultimaAlerta.current[epc] = ahora;
                const info = catalogoRef.current[epc];
                setAlertas(prev => [{
                    id: ahora,
                    epc,
                    descripcion: info?.descripcion || "Equipo desconocido",
                    hora: new Date().toLocaleTimeString(),
                }, ...prev].slice(0, 50));
            });
        };

        // Batch processor — actualiza pantalla cada 100ms
        const intervalo = setInterval(() => {
            if (pendientes.current.length === 0) return;
            const lote = [...pendientes.current];
            pendientes.current = [];
            playBeep();
            const last = lote[lote.length - 1];
            setUltimoTag({ epc: last.epc, antena: last.antena, timestamp: Date.now() });
            setTags(prev => {
                const nuevo = { ...prev };
                for (const { epc, antena } of lote) {
                    const info = catalogoRef.current[epc] || {};
                    nuevo[epc] = {
                        epc,
                        antena,
                        count: (nuevo[epc]?.count || 0) + 1,
                        lastSeen: new Date().toLocaleTimeString(),
                        descripcion: info.descripcion || "",
                        marca:       info.marca       || "",
                        modelo:      info.modelo      || "",
                        categoria:   info.categoria   || "",
                        tipo:        info.tipo        || "",
                    };
                }
                return nuevo;
            });
        }, 100);

        setup();

        return () => {
            clearInterval(intervalo);
            [unTag, unEstado, unLps, unContador, unAlerta].forEach(u => {
                if (u) u.then?.(fn => fn());
            });
        };
    }, []);

    // ── Acciones ──────────────────────────────────────────────────────────────
    async function iniciarLectura() {
        setLeyendo(true);
        setTags({});
        setUltimoTag(null);
        setContadorBackend(0);
        contadorSeg.current = 0;
        lastTime.current = Date.now();
        try {
            await invoke("iniciar_lectura");
        } catch (e) {
            setEstado("error: " + e);
            setLeyendo(false);
        }
    }

    async function detenerLectura() {
        await invoke("detener_lectura");
        setLeyendo(false);
    }

    async function sincronizar() {
        setSincronizando(true);
        setMensajeSync("");
        try {
            const r = await invoke("sincronizar");
            setMensajeSync(r);
            await cargarCatalogo(); // refrescar catálogo tras sync
        } catch (e) {
            setMensajeSync("❌ Error: " + e);
        } finally {
            setSincronizando(false);
        }
    }

    function toggleCategoria(cat) {
        setCategoriasFiltro(prev => {
            const next = new Set(prev);
            next.has(cat) ? next.delete(cat) : next.add(cat);
            return next;
        });
    }

    function toggleAntena(id) {
        setAntenasFiltro(prev => {
            const next = new Set(prev);
            next.has(id) ? next.delete(id) : next.add(id);
            return next;
        });
    }

    // ── Lista filtrada ────────────────────────────────────────────────────────
    const tagList = Object.values(tags)
        .filter(t => {
            if (antenasFiltro.size > 0 && t.antena && !antenasFiltro.has(t.antena)) return false;
            if (categoriasFiltro.size > 0 && !categoriasFiltro.has(t.categoria)) return false;
            if (busqueda) {
                const q = busqueda.toLowerCase();
                return t.epc.toLowerCase().includes(q) || t.descripcion.toLowerCase().includes(q);
            }
            return true;
        })
        .sort((a, b) => b.count - a.count);

    const totalLecturas = tagList.reduce((s, t) => s + t.count, 0);

    // ── Render ────────────────────────────────────────────────────────────────
    const estadoColor = estado === "conectado" ? "#6bff8e"
        : estado === "reconectando" ? "#ffaa44" : "#555";

    return (
        <div style={s.root}>

            {/* ── HEADER ── */}
            <div style={s.header}>
                <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                    <div style={s.dot(estadoColor)} />
                    <span style={s.title}>RFID Monitor</span>
                    <span style={{ ...s.badge, background: "#1a1a1a", color: "#555" }}>
                        {estado}
                    </span>
                </div>
                <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
                    {lps > 0 && (
                        <span style={{ color: lps > 20 ? "#6bff8e" : "#ffaa44", fontSize: 12, fontWeight: "bold" }}>
                            ⚡ {lps}/seg
                        </span>
                    )}
                    {contadorBackend > 0 && (
                        <span style={{ color: "#444", fontSize: 11 }}>Total: {contadorBackend}</span>
                    )}
                    <button
                        onClick={() => setPanelAbierto(p => !p)}
                        style={{ ...s.btnIcon, background: alertas.length ? "#3d1a1a" : "#111", position: "relative" }}
                    >
                        🚨
                        {alertas.length > 0 && (
                            <span style={s.alertBadge}>{alertas.length}</span>
                        )}
                    </button>
                </div>
            </div>

            {/* ── ÚLTIMO TAG ── */}
            {ultimoTag && (() => {
                const info = catalogoRef.current[ultimoTag.epc] || {};
                const badge = tipoBadge(info.tipo);
                return (
                    <div style={s.ultimoTag}>
                        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
                            {/* Lado izquierdo: nombre grande + marca/modelo */}
                            <div style={{ flex: 1, minWidth: 0 }}>
                                <div style={{ fontSize: 10, color: "#333", letterSpacing: 1, marginBottom: 4 }}>
                                    ÚLTIMO TAG LEÍDO
                                </div>
                                {info.descripcion ? (
                                    <>
                                        <div style={{ fontSize: 20, fontWeight: "700", color: "#fff", lineHeight: 1.2, marginBottom: 4 }}>
                                            {info.descripcion}
                                        </div>
                                        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                                            {info.marca && (
                                                <span style={{ fontSize: 12, color: "#888" }}>{info.marca}</span>
                                            )}
                                            {info.marca && info.modelo && (
                                                <span style={{ color: "#2a2a2a", fontSize: 12 }}>·</span>
                                            )}
                                            {info.modelo && (
                                                <span style={{ fontSize: 12, color: "#666" }}>{info.modelo}</span>
                                            )}
                                            {info.tipo && (
                                                <span style={{
                                                    padding: "2px 8px", borderRadius: 3,
                                                    fontSize: 10, fontWeight: "600",
                                                    background: badge.bg, color: badge.color,
                                                }}>
                                                    {badge.text}
                                                </span>
                                            )}
                                        </div>
                                    </>
                                ) : (
                                    <div style={{ fontSize: 16, fontWeight: "700", color: "#ff6b6b", letterSpacing: 1 }}>
                                        Sin registro en catálogo
                                    </div>
                                )}
                                <div style={{ marginTop: 6, fontFamily: "monospace", fontSize: 11, color: "#333" }}>
                                    {ultimoTag.epc}
                                </div>
                            </div>
                            {/* Lado derecho: antena + categoría */}
                            <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 6, marginLeft: 16 }}>
                                {ultimoTag.antena && (
                                    <span style={{ ...s.badge, background: "#1a1a3d", color: "#6b9fff" }}>
                                        Antena {ultimoTag.antena}
                                    </span>
                                )}
                                {info.categoria && (
                                    <span style={{ ...s.badge, background: "#1a1a1a", color: "#444" }}>
                                        {info.categoria}
                                    </span>
                                )}
                            </div>
                        </div>
                    </div>
                );
            })()}

            {/* ── CONTROLES PRINCIPALES ── */}
            <div style={s.row}>
                <button onClick={iniciarLectura} disabled={leyendo}
                    style={{ ...s.btn, background: leyendo ? "#222" : "#c0392b", color: leyendo ? "#555" : "white" }}>
                    {leyendo ? "⏳ Leyendo…" : "▶ Iniciar"}
                </button>
                <button onClick={detenerLectura} disabled={!leyendo}
                    style={{ ...s.btn, background: !leyendo ? "#222" : "#922b21", color: !leyendo ? "#555" : "white" }}>
                    ⏹ Detener
                </button>
                <button onClick={() => { setTags({}); setUltimoTag(null); }}
                    style={{ ...s.btn, background: "#1a1a1a" }}>
                    🗑 Limpiar
                </button>
                <button onClick={sincronizar} disabled={sincronizando || leyendo}
                    style={{ ...s.btn, background: sincronizando ? "#222" : "#1a3a5c", color: sincronizando ? "#555" : "white" }}>
                    {sincronizando ? "⏳ Sincronizando…" : "🔄 Sincronizar"}
                </button>
            </div>

            {mensajeSync && (
                <div style={s.toast}>{mensajeSync}</div>
            )}

            {/* ── FILTROS ── */}
            <div style={s.filtrosPanel}>

                {/* Antenas */}
                <div style={s.filtroGrupo}>
                    <span style={s.filtroLabel}>ANTENAS</span>
                    <div style={{ display: "flex", gap: 6 }}>
                        {ANTENAS.map(a => (
                            <button key={a.id}
                                onClick={() => toggleAntena(a.id)}
                                style={{
                                    ...s.chip,
                                    background: antenasFiltro.has(a.id) ? "#1a2d4a" : "#111",
                                    color: antenasFiltro.has(a.id) ? "#6b9fff" : "#444",
                                    borderColor: antenasFiltro.has(a.id) ? "#6b9fff" : "#222",
                                }}
                                title={a.desc}
                            >
                                {a.label}
                            </button>
                        ))}
                    </div>
                </div>

                {/* Categorías */}
                {categorias.length > 0 && (
                    <div style={s.filtroGrupo}>
                        <span style={s.filtroLabel}>CATEGORÍAS</span>
                        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                            <button
                                onClick={() => setCategoriasFiltro(new Set())}
                                style={{
                                    ...s.chip,
                                    background: categoriasFiltro.size === 0 ? "#2d2d1a" : "#111",
                                    color: categoriasFiltro.size === 0 ? "#ffee6b" : "#444",
                                    borderColor: categoriasFiltro.size === 0 ? "#ffee6b" : "#222",
                                }}
                            >
                                Todas
                            </button>
                            {categorias.map(cat => (
                                <button key={cat}
                                    onClick={() => toggleCategoria(cat)}
                                    style={{
                                        ...s.chip,
                                        background: categoriasFiltro.has(cat) ? "#2d1a2d" : "#111",
                                        color: categoriasFiltro.has(cat) ? "#d06bff" : "#444",
                                        borderColor: categoriasFiltro.has(cat) ? "#d06bff" : "#222",
                                    }}
                                >
                                    {cat}
                                </button>
                            ))}
                        </div>
                    </div>
                )}

                {/* Búsqueda */}
                <div style={s.filtroGrupo}>
                    <span style={s.filtroLabel}>BUSCAR</span>
                    <input
                        value={busqueda}
                        onChange={e => setBusqueda(e.target.value)}
                        placeholder="EPC o nombre de equipo…"
                        style={s.input}
                    />
                </div>
            </div>

            {/* ── STATS ── */}
            <div style={s.statsRow}>
                {[
                    { n: totalLecturas.toLocaleString(), l: "Lecturas" },
                    { n: tagList.length, l: "Tags únicos" },
                    { n: tagList.length > 0 ? (totalLecturas / tagList.length).toFixed(1) : 0, l: "Promedio/tag" },
                    { n: alertas.length, l: "Alertas" },
                ].map(({ n, l }) => (
                    <div key={l} style={s.statBox}>
                        <span style={s.statNum}>{n}</span>
                        <span style={s.statLbl}>{l}</span>
                    </div>
                ))}
            </div>

            {/* ── TABLA ── */}
            <div style={s.table}>
                {/* Cabecera */}
                <div style={{ ...s.tableRow, background: "#0d0d0d", borderBottom: "1px solid #1f1f1f" }}>
                    <span style={{ ...s.col.num,    color: "#333" }}>#</span>
                    <span style={{ ...s.col.epc,    color: "#333" }}>EPC</span>
                    <span style={{ ...s.col.nombre, color: "#333" }}>NOMBRE</span>
                    <span style={{ ...s.col.marca,  color: "#333" }}>MARCA / MODELO</span>
                    <span style={{ ...s.col.cat,    color: "#333" }}>CATEGORÍA</span>
                    <span style={{ ...s.col.ant,    color: "#333" }}>ANT.</span>
                    <span style={{ ...s.col.cnt,    color: "#333" }}>COUNT</span>
                    <span style={{ ...s.col.hora,   color: "#333" }}>HORA</span>
                </div>

                {/* Body */}
                <div style={{ maxHeight: 420, overflowY: "auto" }}>
                    {tagList.length === 0 ? (
                        <div style={{ padding: "40px 0", textAlign: "center", color: "#2a2a2a", fontSize: 12 }}>
                            {leyendo ? "Acerca un tag a la antena…" : "Presiona Iniciar para comenzar"}
                        </div>
                    ) : (
                        tagList.slice(0, 100).map((tag, idx) => {
                            const esUltimo = ultimoTag?.epc === tag.epc;
                            const badge = tipoBadge(tag.tipo);
                            return (
                                <div key={tag.epc} style={{
                                    ...s.tableRow,
                                    background: esUltimo ? "#1a0f0f" : "transparent",
                                    borderBottom: "1px solid #111",
                                    transition: "background 0.15s",
                                }}>
                                    <span style={{ ...s.col.num, color: "#333" }}>{idx + 1}</span>
                                    <span style={{ ...s.col.epc, color: "#6bff8e", fontFamily: "monospace", fontSize: 11 }}>
                                        {tag.epc}
                                    </span>
                                    {/* NOMBRE + badge tipo */}
                                    <span style={{ ...s.col.nombre, color: "#ccc", fontSize: 12 }}>
                                        {tag.descripcion ? (
                                            <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                                                {tag.descripcion}
                                                {tag.tipo && (
                                                    <span style={{
                                                        padding: "1px 5px", borderRadius: 3,
                                                        fontSize: 9, fontWeight: "600",
                                                        background: badge.bg, color: badge.color,
                                                        flexShrink: 0,
                                                    }}>
                                                        {badge.text}
                                                    </span>
                                                )}
                                            </span>
                                        ) : (
                                            <span style={{ color: "#2a2a2a" }}>—</span>
                                        )}
                                    </span>
                                    {/* MARCA / MODELO */}
                                    <span style={{ ...s.col.marca, fontSize: 11 }}>
                                        {tag.marca || tag.modelo ? (
                                            <span>
                                                <span style={{ color: "#888" }}>{tag.marca}</span>
                                                {tag.marca && tag.modelo && (
                                                    <span style={{ color: "#2a2a2a" }}> / </span>
                                                )}
                                                <span style={{ color: "#555" }}>{tag.modelo}</span>
                                            </span>
                                        ) : (
                                            <span style={{ color: "#2a2a2a" }}>—</span>
                                        )}
                                    </span>
                                    <span style={{ ...s.col.cat, color: "#555", fontSize: 11 }}>
                                        {tag.categoria || "—"}
                                    </span>
                                    <span style={{ ...s.col.ant, color: "#6b9fff", fontSize: 11 }}>
                                        {tag.antena ? `A${tag.antena}` : "—"}
                                    </span>
                                    <span style={{ ...s.col.cnt, color: "#ff6b6b", fontWeight: "bold", fontSize: 15 }}>
                                        {tag.count}
                                    </span>
                                    <span style={{ ...s.col.hora, color: "#333", fontSize: 11 }}>
                                        {tag.lastSeen}
                                    </span>
                                </div>
                            );
                        })
                    )}
                </div>
            </div>

            {/* ── PANEL ALERTAS ── */}
            {panelAbierto && (
                <div style={s.panel}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 14 }}>
                        <span style={{ color: "#c0392b", fontWeight: "bold", fontSize: 13 }}>
                            🚨 Alertas uso interno ({alertas.length})
                        </span>
                        <button onClick={() => setPanelAbierto(false)}
                            style={{ background: "none", border: "none", color: "#555", cursor: "pointer", fontSize: 18 }}>
                            ✕
                        </button>
                    </div>
                    <div style={{ flex: 1, overflowY: "auto" }}>
                        {alertas.length === 0 ? (
                            <p style={{ color: "#333", fontSize: 12, textAlign: "center" }}>Sin alertas</p>
                        ) : alertas.map(a => (
                            <div key={a.id} style={s.alertCard}>
                                <div style={{ color: "#ff6b6b", fontWeight: "bold", fontSize: 12, marginBottom: 2 }}>
                                    {a.descripcion}
                                </div>
                                <div style={{ color: "#444", fontSize: 10, fontFamily: "monospace" }}>
                                    {a.epc}
                                </div>
                                <div style={{ color: "#555", fontSize: 10, marginTop: 4 }}>
                                    {a.hora} · salida no autorizada
                                </div>
                            </div>
                        ))}
                    </div>
                    {alertas.length > 0 && (
                        <button onClick={() => setAlertas([])} style={s.clearBtn}>
                            Limpiar historial
                        </button>
                    )}
                </div>
            )}
        </div>
    );
}

// ─── ESTILOS ──────────────────────────────────────────────────────────────────
const s = {
    root: {
        padding: 20,
        background: "#080808",
        minHeight: "100vh",
        color: "white",
        fontFamily: "'Segoe UI', system-ui, monospace",
        fontSize: 13,
    },
    header: {
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        marginBottom: 14,
    },
    title: { fontSize: 16, fontWeight: "600", letterSpacing: 0.5 },
    dot: (color) => ({
        width: 8, height: 8, borderRadius: "50%",
        background: color,
        boxShadow: color !== "#555" ? `0 0 6px ${color}` : "none",
    }),
    badge: {
        padding: "2px 8px",
        borderRadius: 4,
        fontSize: 10,
        fontWeight: "600",
        letterSpacing: 0.5,
    },
    alertBadge: {
        position: "absolute", top: -5, right: -5,
        background: "#c0392b", color: "white",
        borderRadius: "50%", width: 16, height: 16,
        fontSize: 10, display: "flex",
        alignItems: "center", justifyContent: "center",
        fontWeight: "bold",
    },
    btnIcon: {
        position: "relative",
        padding: "6px 10px",
        border: "1px solid #222",
        borderRadius: 6,
        cursor: "pointer",
        fontSize: 16,
        color: "white",
    },
    ultimoTag: {
        background: "#0f0f0f",
        border: "1px solid #1f1f1f",
        borderRadius: 8,
        padding: "10px 14px",
        marginBottom: 12,
    },
    row: { display: "flex", gap: 8, marginBottom: 12, flexWrap: "wrap" },
    btn: {
        flex: 1, minWidth: 100,
        padding: "10px 12px",
        border: "none", borderRadius: 6,
        fontWeight: "600", fontSize: 12,
        cursor: "pointer", transition: "opacity 0.15s",
    },
    toast: {
        marginBottom: 10,
        padding: "8px 12px",
        background: "#0d1f0d",
        border: "1px solid #1a3a1a",
        borderRadius: 6,
        fontSize: 11,
        color: "#6bff8e",
        textAlign: "center",
    },
    filtrosPanel: {
        background: "#0d0d0d",
        border: "1px solid #1a1a1a",
        borderRadius: 8,
        padding: 12,
        marginBottom: 12,
        display: "flex",
        flexDirection: "column",
        gap: 10,
    },
    filtroGrupo: { display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" },
    filtroLabel: { fontSize: 10, color: "#333", letterSpacing: 1, minWidth: 80 },
    chip: {
        padding: "4px 10px",
        border: "1px solid",
        borderRadius: 4,
        cursor: "pointer",
        fontSize: 11,
        fontWeight: "600",
        background: "#111",
        transition: "all 0.15s",
    },
    input: {
        flex: 1, minWidth: 180,
        padding: "5px 10px",
        background: "#111",
        border: "1px solid #222",
        borderRadius: 4,
        color: "white",
        fontSize: 12,
        outline: "none",
    },
    statsRow: { display: "flex", gap: 8, marginBottom: 12 },
    statBox: {
        flex: 1, background: "#0d0d0d",
        border: "1px solid #1a1a1a",
        borderRadius: 6, padding: "8px 10px",
        textAlign: "center",
    },
    statNum: { display: "block", fontSize: 22, fontWeight: "bold", color: "#e74c3c" },
    statLbl: { display: "block", fontSize: 9, color: "#333", letterSpacing: 1, marginTop: 2 },
    table: {
        border: "1px solid #1a1a1a",
        borderRadius: 8,
        overflow: "hidden",
        marginBottom: 12,
    },
    tableRow: { display: "flex", alignItems: "center", padding: "7px 10px" },
    col: {
        num:    { width: 32,  textAlign: "center", flexShrink: 0 },
        epc:    { width: 170, flexShrink: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
        nombre: { flex: 2,    minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
        marca:  { flex: 1,    minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
        cat:    { width: 100, flexShrink: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
        ant:    { width: 44,  textAlign: "center", flexShrink: 0 },
        cnt:    { width: 64,  textAlign: "center", flexShrink: 0 },
        hora:   { width: 72,  textAlign: "right",  flexShrink: 0 },
    },
    panel: {
        position: "fixed", top: 0, right: 0,
        width: 300, height: "100vh",
        background: "#090909",
        borderLeft: "1px solid #2a1a1a",
        zIndex: 1000,
        display: "flex", flexDirection: "column",
        padding: 16,
    },
    alertCard: {
        background: "#110000",
        border: "1px solid #2a1010",
        borderRadius: 6,
        padding: 10,
        marginBottom: 8,
    },
    clearBtn: {
        marginTop: 12, padding: "8px 0",
        background: "#111", border: "1px solid #222",
        color: "#444", borderRadius: 6,
        cursor: "pointer", fontSize: 11, width: "100%",
    },
};
