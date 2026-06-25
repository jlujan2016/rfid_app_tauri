import React, { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./DoorControlDashboard.css";

function DoorControlDashboard() {
  const [isScanning, setIsScanning] = useState(false);
  const [inventory, setInventory] = useState([]);
  const [loadingLocal, setLoadingLocal] = useState(true);
  const [selectedUbicacion, setSelectedUbicacion] = useState("Todas");
  const [toasts, setToasts] = useState([]);

  const inventoryRef = useRef([]);
  const selectedUbicacionRef = useRef("Todas");
  const scanStartTime = useRef(null);
  const hasAlertedInitial = useRef(false);

  // Funciones de notificaciones Toast
  const addToast = (message, type = "info") => {
    const id = Date.now() + Math.random();
    setToasts(prev => [...prev, { id, message, type }]);
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
    }, 5000);
  };

  const cargarInventario = async () => {
    setLoadingLocal(true);
    try {
      const data = await invoke("obtener_inventario_local");
      const initialInv = data.map(item => ({ ...item, status: "missing", lastSeen: null }));
      setInventory(initialInv);
      inventoryRef.current = initialInv;
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
    selectedUbicacionRef.current = selectedUbicacion;
  }, [selectedUbicacion]);

  // Manejo de Etiquetas Leídas
  useEffect(() => {
    const unlistenTag = listen("tag_leido", (event) => {
      const payload = event.payload;
      const epc = typeof payload === "string" ? payload : payload.epc;
      const epc_ascii = payload && payload.epc_ascii ? String(payload.epc_ascii).trim().toLowerCase() : "";
      
      setInventory(prev => {
        let matched = false;
        const newInv = prev.map(item => {
          const dbCode = item.codigo_rfid ? String(item.codigo_rfid).trim().toLowerCase() : "";
          const scanCode = epc ? String(epc).trim().toLowerCase() : "";
          const matchHex = dbCode && scanCode && (scanCode.includes(dbCode) || dbCode.includes(scanCode));
          const matchAscii = dbCode && epc_ascii && (epc_ascii.includes(dbCode) || dbCode.includes(epc_ascii));
          
          if (matchHex || matchAscii) {
            matched = true;
            return { ...item, status: "found", lastSeen: Date.now() };
          }
          return item;
        });
        
        if (matched) {
          inventoryRef.current = newInv;
          return newInv;
        }
        return prev;
      });
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

  // Intervalo de Verificación Constante
  useEffect(() => {
    let interval;
    if (isScanning) {
      interval = setInterval(() => {
        const now = Date.now();
        
        // 1. Verificación inicial a los 15 segundos
        if (!hasAlertedInitial.current && scanStartTime.current && now - scanStartTime.current > 15000) {
           hasAlertedInitial.current = true;
           const missingCount = inventoryRef.current.filter(i => 
              (selectedUbicacionRef.current === "Todas" || (i.estado_ubicacion || "Sin Ubicación") === selectedUbicacionRef.current) && 
              i.status !== "found"
           ).length;
           
           if (missingCount > 0) {
              addToast(`⚠️ Faltan ${missingCount} equipos en la lectura inicial (pasaron 15s).`, "error");
           } else {
              addToast(`✅ Lectura inicial completada. Todos los equipos presentes.`, "success");
           }
        }

        // 2. Verificación continua de "Salida de rango" (10 segundos)
        let updated = false;
        const newInv = inventoryRef.current.map(item => {
           if (item.status === "found" && item.lastSeen && (now - item.lastSeen > 10000)) {
              addToast(`🚨 El equipo "${item.item}" ha salido del rango de lectura.`, "warning");
              updated = true;
              return { ...item, status: "missing", lastSeen: null };
           }
           return item;
        });

        if (updated) {
           setInventory(newInv);
           inventoryRef.current = newInv;
        }

      }, 1000);
    }
    return () => clearInterval(interval);
  }, [isScanning]);

  const toggleScan = async () => {
    try {
      if (!isScanning) {
        setIsScanning(true);
        scanStartTime.current = Date.now();
        hasAlertedInitial.current = false;
        
        const resetInv = inventory.map(item => {
          if (selectedUbicacion === "Todas" || (item.estado_ubicacion || "Sin Ubicación") === selectedUbicacion) {
             return { ...item, status: "missing", lastSeen: null };
          }
          return item;
        });
        setInventory(resetInv);
        inventoryRef.current = resetInv;

        await invoke("iniciar_lectura", { antena: 1 }); // Pin 2
      } else {
        await invoke("detener_lectura");
      }
    } catch (error) {
      addToast("Error interactuando con el lector: " + error, "error");
      setIsScanning(false);
    }
  };

  const ubicaciones = ["Todas", ...new Set(inventory.map(item => item.estado_ubicacion || "Sin Ubicación").filter(Boolean))];
  
  const filteredInventory = inventory.filter(item => 
    selectedUbicacion === "Todas" || (item.estado_ubicacion || "Sin Ubicación") === selectedUbicacion
  );

  const totalItems = filteredInventory.length;
  const foundItems = filteredInventory.filter(i => i.status === "found").length;
  const missingItems = totalItems - foundItems;
  const progressPercent = totalItems === 0 ? 0 : Math.round((foundItems / totalItems) * 100);

  return (
    <div className="door-dashboard-container">
      {/* Sistema de Notificaciones Flotantes (Toasts) */}
      <div style={{ position: "fixed", top: "20px", right: "20px", zIndex: 9999, display: "flex", flexDirection: "column", gap: "10px" }}>
        {toasts.map(toast => (
          <div key={toast.id} style={{
            background: toast.type === 'error' ? 'rgba(231, 76, 60, 0.95)' : toast.type === 'warning' ? 'rgba(243, 156, 18, 0.95)' : toast.type === 'success' ? 'rgba(46, 204, 113, 0.95)' : 'rgba(52, 73, 94, 0.95)',
            color: 'white',
            padding: '12px 20px',
            borderRadius: '8px',
            boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
            animation: 'fadeIn 0.3s ease-out',
            maxWidth: '350px',
            fontWeight: 'bold',
            backdropFilter: 'blur(4px)'
          }}>
            {toast.message}
          </div>
        ))}
      </div>

      <header className="door-dashboard-header">
        <h1 className="door-dashboard-title">Monitoreo en Tiempo Real</h1>
        <div style={{ display: "flex", alignItems: "center", gap: "20px" }}>
          
          <select
            value={selectedUbicacion}
            onChange={(e) => setSelectedUbicacion(e.target.value)}
            style={{
              padding: "8px 16px",
              background: "rgba(255, 255, 255, 0.05)",
              color: "#fff",
              border: "1px solid rgba(255, 255, 255, 0.2)",
              borderRadius: "8px",
              cursor: "pointer",
              outline: "none"
            }}
          >
            {ubicaciones.map(ub => (
              <option key={ub} value={ub} style={{ color: "black" }}>{ub}</option>
            ))}
          </select>

          <button 
            className={`scan-button ${isScanning ? 'active' : ''}`}
            onClick={toggleScan}
          >
            {isScanning ? "🛑 Detener Monitoreo" : "▶️ Iniciar Monitoreo"}
          </button>
          
          <div style={{ display: "flex", alignItems: "center", gap: "10px", color: "#94a3b8" }}>
             <span>{isScanning ? "Monitoreando..." : "Detenido"}</span>
             <div style={{ width: "12px", height: "12px", borderRadius: "50%", background: isScanning ? "#2ecc71" : "#e74c3c", boxShadow: isScanning ? "0 0 10px #2ecc71" : "none" }}></div>
          </div>
        </div>
      </header>

      <section style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "1.5rem", marginBottom: "2rem" }}>
        <div className="stat-card">
          <span className="stat-label">Total en Ubicación</span>
          <span className="stat-value">{totalItems}</span>
        </div>
        <div className="stat-card" style={{ borderColor: foundItems > 0 ? "rgba(46, 204, 113, 0.3)" : ""}}>
          <span className="stat-label">Presentes</span>
          <span className="stat-value" style={{ color: foundItems > 0 ? "#2ecc71" : "white"}}>{foundItems}</span>
        </div>
        <div className="stat-card" style={{ borderColor: missingItems > 0 ? "rgba(231, 76, 60, 0.3)" : ""}}>
          <span className="stat-label">Faltantes</span>
          <span className="stat-value" style={{ color: missingItems > 0 ? "#e74c3c" : "white"}}>{missingItems}</span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Cobertura</span>
          <span className="stat-value">{progressPercent}%</span>
          <div style={{ width: '100%', height: '4px', background: '#333', borderRadius: '2px', marginTop: '10px' }}>
             <div style={{ width: `${progressPercent}%`, height: '100%', background: '#ff4b4b', borderRadius: '2px', transition: 'width 0.3s ease' }}></div>
          </div>
        </div>
      </section>

      <main className="door-dashboard-main">
        <div className="door-inventory-section" style={{ flex: 1 }}>
          <h2>Lista de Equipos ({filteredInventory.length})</h2>
          
          {loadingLocal ? (
            <div style={{ padding: "2rem", color: "#94a3b8" }}>Cargando datos...</div>
          ) : (
            <div className="door-table-wrapper">
              <table className="door-table">
                <thead>
                  <tr>
                    <th>Item / EPC</th>
                    <th>Estado</th>
                    <th>Última Lectura</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredInventory.map(item => {
                    const isFound = item.status === "found";
                    const secondsAgo = item.lastSeen ? Math.floor((Date.now() - item.lastSeen) / 1000) : null;
                    return (
                      <tr key={item.codigo_rfid}>
                        <td>
                          <strong style={{color:"#fff"}}>{item.item || "Desconocido"}</strong>
                          <div style={{fontSize: "0.75rem", color: "#94a3b8"}}>{item.codigo_rfid}</div>
                        </td>
                        <td>
                          <span className={`status-badge ${isFound ? 'found' : 'missing'}`}>
                            {isFound ? "EN RANGO" : "FUERA DE RANGO"}
                          </span>
                        </td>
                        <td style={{ color: "#94a3b8" }}>
                          {isFound && secondsAgo !== null 
                            ? `Hace ${secondsAgo}s` 
                            : "-"}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </main>
    </div>
  );
}

export default DoorControlDashboard;
