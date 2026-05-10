import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as THREE from "three";

function Login({ onSuccess }) {
  const mountRef = useRef(null);
  const logoRef = useRef(null);

  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");

  useEffect(() => {
    let scene = new THREE.Scene();

    let camera = new THREE.PerspectiveCamera(
      75,
      window.innerWidth / window.innerHeight,
      0.1,
      1000
    );

    let renderer = new THREE.WebGLRenderer({ alpha: true });
    renderer.setSize(window.innerWidth, window.innerHeight);

    if (mountRef.current) {
      mountRef.current.appendChild(renderer.domElement);
    }

    // 🔴 GRID
    const grid = new THREE.GridHelper(50, 50, 0xff0000, 0x444444);
    scene.add(grid);

    // 🔴 RADAR (círculo)
    const geometry = new THREE.RingGeometry(1.5, 1.7, 64);
    const material = new THREE.MeshBasicMaterial({
      color: 0xff0000,
      side: THREE.DoubleSide,
    });

    const ring = new THREE.Mesh(geometry, material);
    scene.add(ring);

    // ⚪ Línea radar
    const lineMaterial = new THREE.LineBasicMaterial({ color: 0xffffff });
    const points = [
      new THREE.Vector3(0, 0, 0),
      new THREE.Vector3(2, 0, 0),
    ];

    const lineGeometry = new THREE.BufferGeometry().setFromPoints(points);
    const radarLine = new THREE.Line(lineGeometry, lineMaterial);
    scene.add(radarLine);

    camera.position.z = 5;

    // 🔥 ANIMACIÓN
    let frameId;
    let time = 0;

    const animate = () => {
      frameId = requestAnimationFrame(animate);

      time += 0.05;

      grid.rotation.z += 0.001;
      ring.rotation.z += 0.01;
      radarLine.rotation.z += 0.05;

      // 🔥 PULSO DEL LOGO
      if (logoRef.current) {
        const scale = 1 + Math.sin(time) * 0.1;

        logoRef.current.style.transform = `scale(${scale})`;
        logoRef.current.style.filter =
          "drop-shadow(0 0 " + (10 + Math.sin(time) * 10) + "px red)";
      }

      renderer.render(scene, camera);
    };

    animate();

    return () => {
      cancelAnimationFrame(frameId);
      try {
        if (mountRef.current && renderer.domElement) {
          mountRef.current.removeChild(renderer.domElement);
        }
        renderer.dispose();
      } catch (e) {
        console.warn("Cleanup Three.js:", e);
      }
    };
  }, []);

  async function handleLogin() {
    const ok = await invoke("login", { user, pass });

    if (ok) {
      onSuccess();
    } else {
      alert("Credenciales incorrectas");
    }
  }

  return (
    <div
      style={{
        position: "relative",
        height: "100vh",
        background: "#0a0a0a",
      }}
    >
      {/* 🔥 THREE BACKGROUND */}
      <div
        ref={mountRef}
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          zIndex: 0,
        }}
      />

      {/* 🖥️ PANEL */}
      <div
        style={{
          position: "relative",
          zIndex: 1,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          color: "#ffffff",
          fontFamily: "monospace",
        }}
      >
        <div
          style={{
            border: "1px solid red",
            padding: "30px",
            background: "rgba(0,0,0,0.85)",
            boxShadow: "0 0 20px red",
            width: "300px",
          }}
        >
          {/* 🔴 LOGO CON PULSO */}
          <img
            ref={logoRef}
            src="/glefperu.png"
            alt="logo"
            style={{
              width: "90px",
              display: "block",
              margin: "0 auto 15px",
              transition: "transform 0.1s linear",
            }}
          />

          <h2 style={{ textAlign: "center", color: "white" }}>
            RFID SYSTEM
          </h2>

          <input
            placeholder="USER"
            onChange={(e) => setUser(e.target.value)}
            style={inputStyle}
          />

          <input
            type="password"
            placeholder="PASSWORD"
            onChange={(e) => setPass(e.target.value)}
            style={inputStyle}
          />

          <button onClick={handleLogin} style={buttonStyle}>
            ACCESS
          </button>
        </div>
      </div>
    </div>
  );
}

const inputStyle = {
  width: "100%",
  margin: "10px 0",
  padding: "10px",
  background: "#000",
  border: "1px solid red",
  color: "white",
};

const buttonStyle = {
  width: "100%",
  padding: "10px",
  background: "red",
  border: "none",
  color: "white",
  cursor: "pointer",
  fontWeight: "bold",
};

export default Login;