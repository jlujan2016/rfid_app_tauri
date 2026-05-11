import { useState } from "react";
import "./App.css";
import Login from "./login";
import InventoryDashboard from "./InventoryDashboard";

function App() {
  const [logged, setLogged] = useState(false);

  if (!logged) {
    return <Login onSuccess={() => setLogged(true)} />;
  }

  return <InventoryDashboard />;
}

export default App;