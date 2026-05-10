
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import Login from "./Login";
import RfidTest from "./RfidTest";

function App() {
  const [logged, setLogged] = useState(false);

  if (!logged) {
    return <Login onSuccess={() => setLogged(true)} />;
  }

  return <RfidTest />;
}

export default App;