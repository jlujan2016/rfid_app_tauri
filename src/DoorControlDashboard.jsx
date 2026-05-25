import React, { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./DoorControlDashboard.css";

function DoorControlDashboard() {
  const [isScanning, setIsScanning] = useState(false);
  const [inventory, setInventory] = useState([]);
  const [events, setEvents] = useState([]); // Log de movimientos
  const [loadingLocal, setLoadingLocal] = useState(true);
  const [passwordPrompt, setPasswordPrompt] = useState({ active: false, targetEpc: null });
  const [passwordInput, setPasswordInput] = useState("");
  
  const lastReadTimes = useRef(new Map()); // Anti-rebote por EPC (tiempo ms)
  const ANTI_BOUNCE_MS = 5000; // 5 segundos de espera antes de volver a registrar cruce del mismo equipo

  const cargarInventario = async () => {
    setLoadingLocal(true);
    try {
      const data = await invoke("obtener_inventario_local");
      setInventory(data);
    } catch (error) {
      console.error("Error cargando inventario local:", error);
    } finally {
      setLoadingLocal(false);
    }
  };

  useEffect(() => {
    cargarInventario();
  }, []);

  useEffect(() => {
    const unlistenTag = listen("tag_leido", async (event) => {
      const epc = event.payload;
      const now = Date.now();
      
      // Anti-rebote
      if (lastReadTimes.current.has(epc)) {
        if (now - lastReadTimes.current.get(epc) < ANTI_BOUNCE_MS) {
          return; // Ignorar lecturas repetidas muy rápido
        }
      }
      lastReadTimes.current.set(epc, now);

      try {
        // Registrar cruce en el backend
        const resultado = await invoke("registrar_cruce_puerta", { codigoRfid: epc });
        
        // Actualizar UI
        setEvents(prev => [{
          id: Date.now(),
          time: new Date(),
          ...resultado
        }, ...prev].slice(0, 50)); // Mantener max 50 eventos
        
        // Actualizar tabla en vivo
        setInventory(prev => prev.map(item => 
          item.codigo_rfid === epc ? { ...item, estado_ubicacion: resultado.nuevo_estado } : item
        ));

      } catch (error) {
         console.warn("Error o tag no registrado:", error);
      }
    });

    const unlistenEstado = listen("rfid_estado", (event) => {
      if (event.payload === "detenido") setIsScanning(false);
      else if (event.payload === "conectado") setIsScanning(true);
    });

    const unlistenPromises = [unlistenTag, unlistenEstado];
    return () => {
      unlistenPromises.forEach((fn) => fn.then((f) => f()));
    };
  }, []);

  const toggleScan = async () => {
    try {
      if (!isScanning) {
        await invoke("iniciar_lectura");
      } else {
        await invoke("detener_lectura");
      }
    } catch (error) {
      alert("Error interactuando con el lector: " + error);
      setIsScanning(false);
    }
  };

  const handleTogglePermiso = async (e) => {
    e.preventDefault();
    try {
      await invoke("cambiar_permiso_salida", { 
        codigoRfid: passwordPrompt.targetEpc, 
        adminPass: passwordInput 
      });
      // Actualizar local
      setInventory(prev => prev.map(item => 
        item.codigo_rfid === passwordPrompt.targetEpc 
          ? { ...item, permiso_salida: !item.permiso_salida } 
          : item
      ));
      setPasswordPrompt({ active: false, targetEpc: null });
      setPasswordInput("");
    } catch (error) {
      alert("Error: " + error);
    }
  };

  return (
    <div className="door-dashboard-container">
      <header className="door-dashboard-header">
        <h1 className="door-dashboard-title">Panel de Control de Puerta</h1>
        <div style={{ display: "flex", alignItems: "center", gap: "20px" }}>
          <button 
            className={`scan-button ${isScanning ? 'active' : ''}`}
            onClick={toggleScan}
          >
            {isScanning ? "🛑 Detener Puerta" : "▶️ Activar Lector de Puerta"}
          </button>
          <div style={{ display: "flex", alignItems: "center", gap: "10px", color: "#94a3b8" }}>
             <span>{isScanning ? "Escaneando..." : "Detenido"}</span>
             <div style={{ width: "12px", height: "12px", borderRadius: "50%", background: isScanning ? "#2ecc71" : "#e74c3c", boxShadow: isScanning ? "0 0 10px #2ecc71" : "none" }}></div>
          </div>
        </div>
      </header>

      <main className="door-dashboard-main">
        {/* Lado izquierdo: Lista de Equipos */}
        <div className="door-inventory-section">
          <h2>Equipos ({inventory.length})</h2>
          
          {loadingLocal ? (
            <div style={{ padding: "2rem", color: "#94a3b8" }}>Cargando datos...</div>
          ) : (
            <div className="door-table-wrapper">
              <table className="door-table">
                <thead>
                  <tr>
                    <th>Item</th>
                    <th>Ubicación</th>
                    <th>¿Puede Salir?</th>
                    <th>Acción</th>
                  </tr>
                </thead>
                <tbody>
                  {inventory.map(item => (
                    <tr key={item.codigo_rfid}>
                      <td>
                        <strong style={{color:"#fff"}}>{item.item || "Desconocido"}</strong>
                        <div style={{fontSize: "0.75rem", color: "#94a3b8"}}>{item.codigo_rfid}</div>
                      </td>
                      <td>
                        <span className={`status-badge ${item.estado_ubicacion === 'Afuera' ? 'missing' : 'found'}`}>
                          {item.estado_ubicacion || "En Oficina"}
                        </span>
                      </td>
                      <td>
                        <span style={{ color: item.permiso_salida ? "#2ecc71" : "#e74c3c", fontWeight: "bold" }}>
                           {item.permiso_salida ? "SÍ" : "NO"}
                        </span>
                      </td>
                      <td>
                        {passwordPrompt.active && passwordPrompt.targetEpc === item.codigo_rfid ? (
                          <form onSubmit={handleTogglePermiso} className="password-form">
                            <input 
                              type="password" 
                              placeholder="Admin pass" 
                              value={passwordInput} 
                              onChange={e => setPasswordInput(e.target.value)} 
                              autoFocus
                            />
                            <button type="submit" className="btn-ok">OK</button>
                            <button type="button" className="btn-cancel" onClick={() => setPasswordPrompt({active: false, targetEpc: null})}>X</button>
                          </form>
                        ) : (
                          <button 
                            className="btn-change-perm" 
                            onClick={() => { setPasswordPrompt({active: true, targetEpc: item.codigo_rfid}); setPasswordInput(""); }}
                          >
                            Cambiar
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Lado derecho: Registro de Eventos (Cruces) */}
        <div className="door-events-section">
          <h2>Registro de Cruces</h2>
          <div className="events-list">
            {events.length === 0 ? (
              <div style={{ color: "#64748b", padding: "1rem", textAlign: "center" }}>No hay cruces recientes.</div>
            ) : (
              events.map(ev => (
                <div key={ev.id} className={`event-card ${ev.alarma ? 'alarm' : ev.nuevo_estado === 'En Oficina' ? 'entry' : 'exit'}`}>
                  <div className="event-time">{ev.time.toLocaleTimeString()}</div>
                  <div className="event-details">
                    <strong>{ev.nombre_item}</strong><br/>
                    {ev.alarma ? (
                       <span style={{ color: "#ef4444" }}>⚠️ ALARMA: Salió sin permiso</span>
                    ) : (
                       <span>{ev.estado_anterior} ➝ {ev.nuevo_estado}</span>
                    )}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

export default DoorControlDashboard;
