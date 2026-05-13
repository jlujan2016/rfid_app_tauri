import { useState } from "react";
import "./App.css";
import Login from "./login";
import InventoryDashboard from "./InventoryDashboard";
import RfidTest from "./RfidTest";
import RfidSimulation from "./RfidSimulation";

function App() {
  const [logged, setLogged] = useState(false);
  const [activeTab, setActiveTab] = useState("dashboard");

  if (!logged) {
    return <Login onSuccess={() => setLogged(true)} />;
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', width: '100vw' }}>
      <nav style={{ background: '#0f172a', padding: '10px 20px', display: 'flex', gap: '15px', borderBottom: '1px solid #1e293b' }}>
        <button 
          onClick={() => setActiveTab("dashboard")}
          style={{ padding: '8px 16px', background: activeTab === "dashboard" ? '#3b82f6' : 'transparent', color: activeTab === "dashboard" ? 'white' : '#94a3b8', border: '1px solid #3b82f6', borderRadius: '4px', cursor: 'pointer', fontWeight: '500' }}
        >
          Dashboard RFID
        </button>
        <button 
          onClick={() => setActiveTab("simulation")}
          style={{ padding: '8px 16px', background: activeTab === "simulation" ? '#3b82f6' : 'transparent', color: activeTab === "simulation" ? 'white' : '#94a3b8', border: '1px solid #3b82f6', borderRadius: '4px', cursor: 'pointer', fontWeight: '500' }}
        >
          Simulación 3D
        </button>
      </nav>
      <div style={{ flex: 1, overflow: activeTab === "dashboard" ? "auto" : "hidden", position: 'relative' }}>
        {activeTab === "dashboard" ? <InventoryDashboard /> : <RfidSimulation />}
      </div>
    </div>
  );
}

export default App;