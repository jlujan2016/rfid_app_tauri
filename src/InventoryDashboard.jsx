import React, { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./InventoryDashboard.css";

function InventoryDashboard() {
  const [isScanning, setIsScanning] = useState(false);
  const [inventory, setInventory] = useState([]);
  const [scanCount, setScanCount] = useState(0);
  const [sincronizando, setSincronizando] = useState(false);
  const [loadingLocal, setLoadingLocal] = useState(true);
  const [selectedAlmacen, setSelectedAlmacen] = useState("Todos");

  // Referencia para el intervalo de simulación
  const scanInterval = useRef(null);

  const cargarInventarioLocal = async () => {
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
    cargarInventarioLocal();
  }, []);

  const sincronizarNube = async () => {
    setSincronizando(true);
    try {
      const msj = await invoke("sincronizar_inventario");
      // Recargar local despues de sincronizar
      await cargarInventarioLocal();
      alert(msj); 
    } catch (error) {
      alert("❌ Error sincronizando: " + error);
    } finally {
      setSincronizando(false);
    }
  };

  // Lógica Real de Escaneo RFID
  useEffect(() => {
    const unlistenTag = listen("tag_leido", (event) => {
      const epc = event.payload;
      
      setInventory(prev => {
        let changed = false;
        const newInv = prev.map(item => {
          // Si el código recibido de la antena coincide exactamente con el código_rfid de la base de datos
          if (item.codigo_rfid === epc && item.status !== "found") {
            changed = true;
            return { ...item, status: "found" };
          }
          return item;
        });

        if (changed) {
          setScanCount(c => c + 1);
        }
        return newInv;
      });
    });

    const unlistenEstado = listen("rfid_estado", (event) => {
      if (event.payload === "detenido") {
        setIsScanning(false);
      } else if (event.payload === "conectado") {
        setIsScanning(true);
      }
    });

    const unlistenPromises = [unlistenTag, unlistenEstado];

    return () => {
      unlistenPromises.forEach((fn) => fn.then((f) => f()));
    };
  }, []);

  // Lista de almacenes únicos
  const almacenes = ["Todos", ...new Set(inventory.map(item => item.almacen || "Sin Almacén").filter(Boolean))];

  // Inventario filtrado
  const filteredInventory = inventory.filter(item => 
    selectedAlmacen === "Todos" || (item.almacen || "Sin Almacén") === selectedAlmacen
  );

  const toggleScan = async () => {
    try {
      if (!isScanning) {
        if (filteredInventory.every(i => i.status === "found")) {
          // Reiniciar si ya se encontró todo en el almacén actual
          setInventory(prev => prev.map(item => {
            if (selectedAlmacen === "Todos" || (item.almacen || "Sin Almacén") === selectedAlmacen) {
               return { ...item, status: "missing" };
            }
            return item;
          }));
          setScanCount(0);
        }
        await invoke("iniciar_lectura");
      } else {
        await invoke("detener_lectura");
      }
    } catch (error) {
      alert("Error interactuando con el lector: " + error);
      setIsScanning(false);
    }
  };

  const totalItems = filteredInventory.length;
  const foundItems = filteredInventory.filter(i => i.status === "found").length;
  const missingItems = totalItems - foundItems;
  const progressPercent = totalItems === 0 ? 0 : Math.round((foundItems / totalItems) * 100);

  return (
    <div className="dashboard-container">
      <header className="dashboard-header">
        <h1 className="dashboard-title">RFID Inventory System</h1>
        
        <div style={{ display: "flex", alignItems: "center", gap: "20px" }}>
          
          <select
            value={selectedAlmacen}
            onChange={(e) => setSelectedAlmacen(e.target.value)}
            style={{
              padding: "8px 16px",
              background: "rgba(255, 255, 255, 0.05)",
              color: "#fff",
              border: "1px solid rgba(255, 255, 255, 0.2)",
              borderRadius: "8px",
              cursor: "pointer",
              outline: "none",
              fontFamily: "inherit"
            }}
          >
            {almacenes.map(al => (
              <option key={al} value={al} style={{ color: "black" }}>{al}</option>
            ))}
          </select>

          <button 
            onClick={sincronizarNube}
            disabled={sincronizando}
            style={{
              padding: "8px 16px",
              background: sincronizando ? "#555" : "rgba(255, 75, 75, 0.2)",
              color: "#fff",
              border: "1px solid #ff4b4b",
              borderRadius: "8px",
              cursor: sincronizando ? "not-allowed" : "pointer"
            }}
          >
            {sincronizando ? "🔄 Sincronizando..." : "☁️ Sincronizar Nube"}
          </button>
          <div style={{ color: "#94a3b8", display: "flex", alignItems: "center", gap: "10px" }}>
            <span>{new Date().toLocaleDateString()}</span>
            <div style={{ width: "10px", height: "10px", borderRadius: "50%", background: isScanning ? "#2ecc71" : "#e74c3c", boxShadow: isScanning ? "0 0 10px #2ecc71" : "none" }}></div>
          </div>
        </div>
      </header>

      <section className="stats-grid">
        <div className="stat-card">
          <span className="stat-label">Total Artículos</span>
          <span className="stat-value">{totalItems}</span>
        </div>
        <div className="stat-card" style={{ borderColor: foundItems > 0 ? "rgba(46, 204, 113, 0.3)" : ""}}>
          <span className="stat-label">Escaneados</span>
          <span className="stat-value" style={{ color: foundItems > 0 ? "#2ecc71" : "white"}}>{foundItems}</span>
        </div>
        <div className="stat-card" style={{ borderColor: missingItems > 0 ? "rgba(231, 76, 60, 0.3)" : ""}}>
          <span className="stat-label">Faltantes</span>
          <span className="stat-value" style={{ color: missingItems > 0 ? "#e74c3c" : "white"}}>{missingItems}</span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Progreso</span>
          <span className="stat-value">{progressPercent}%</span>
          <div style={{ width: '100%', height: '4px', background: '#333', borderRadius: '2px', marginTop: '10px' }}>
             <div style={{ width: `${progressPercent}%`, height: '100%', background: '#ff4b4b', borderRadius: '2px', transition: 'width 0.3s ease' }}></div>
          </div>
        </div>
      </section>

      <main className="main-content">
        <div className="controls-section">
          <button 
            className={`scan-button ${isScanning ? 'active' : ''}`}
            onClick={toggleScan}
            disabled={totalItems === 0}
          >
            {isScanning ? (
              <>
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>
                Detener Escaneo
              </>
            ) : (
              <>
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 3v18M19 3v18M9 3v18M15 3v18"></path></svg>
                {foundItems === totalItems && totalItems > 0 ? "Reiniciar Escaneo" : "Iniciar Escaneo"}
              </>
            )}
          </button>
        </div>

        <div className="table-container">
          {loadingLocal ? (
            <div style={{ padding: "2rem", textAlign: "center", color: "#94a3b8" }}>Cargando inventario local...</div>
          ) : filteredInventory.length === 0 ? (
            <div style={{ padding: "2rem", textAlign: "center", color: "#94a3b8" }}>
              {inventory.length === 0 ? "No hay equipos locales. Presiona 'Sincronizar Nube' para descargarlos." : "No hay equipos en este almacén."}
            </div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="inventory-table">
                <thead>
                  <tr>
                    <th>EPC / Tag ID</th>
                    <th>Item</th>
                    <th>Marca/Modelo</th>
                    <th>Ubicación</th>
                    <th>Estado</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredInventory.map((item) => (
                    <tr key={item.codigo_rfid} className={item.status === "found" ? "scanned" : ""}>
                      <td style={{ fontFamily: "monospace", color: "#a5b4fc" }}>{item.codigo_rfid}</td>
                      <td style={{ fontWeight: "500", color: "#fff" }}>
                        {item.item || "Sin nombre"}
                        {item.descripcion && <div style={{ fontSize: "0.8rem", color: "#94a3b8", fontWeight: "normal" }}>{item.descripcion}</div>}
                      </td>
                      <td>
                        {item.marca || "-"} / {item.modelo || "-"}
                        {item.numero_serie && <div style={{ fontSize: "0.8rem", color: "#94a3b8" }}>S/N: {item.numero_serie}</div>}
                      </td>
                      <td>
                        {item.almacen || "Sin Almacén"}
                        {item.categoria && <div style={{ fontSize: "0.8rem", color: "#94a3b8" }}>Cat: {item.categoria}</div>}
                      </td>
                      <td>
                        <span className={`status-badge ${item.status}`}>
                          {item.status === "found" ? "Encontrado" : "Faltante"}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </main>
    </div>
  );
}

export default InventoryDashboard;
