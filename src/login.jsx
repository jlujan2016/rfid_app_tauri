import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as THREE from "three";

function Login({ onSuccess }) {
  const mountRef = useRef(null);
  const logoRef = useRef(null);
  
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    // Escena principal
    let scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x0a0a0a, 0.05);

    let camera = new THREE.PerspectiveCamera(
      75,
      window.innerWidth / window.innerHeight,
      0.1,
      1000
    );

    let renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true });
    renderer.setSize(window.innerWidth, window.innerHeight);
    renderer.setPixelRatio(window.devicePixelRatio);

    if (mountRef.current) {
      mountRef.current.appendChild(renderer.domElement);
    }

    // Grupos para rotaciones complejas
    const coreGroup = new THREE.Group();
    scene.add(coreGroup);

    // 🔴 GRID (Suelo)
    const grid = new THREE.GridHelper(50, 50, 0xff2222, 0x330000);
    grid.position.y = -2;
    scene.add(grid);

    // 🔴 ANILLOS HOLOGRÁFICOS
    const ringMat1 = new THREE.MeshBasicMaterial({ color: 0xff0033, side: THREE.DoubleSide, wireframe: true, transparent: true, opacity: 0.8 });
    const ringMat2 = new THREE.MeshBasicMaterial({ color: 0xffaaaa, side: THREE.DoubleSide, transparent: true, opacity: 0.3 });
    const ringMat3 = new THREE.MeshBasicMaterial({ color: 0xff0000, side: THREE.DoubleSide, wireframe: true });

    const ring1 = new THREE.Mesh(new THREE.TorusGeometry(2, 0.05, 16, 100), ringMat1);
    const ring2 = new THREE.Mesh(new THREE.TorusGeometry(1.5, 0.02, 16, 100), ringMat2);
    const ring3 = new THREE.Mesh(new THREE.TorusGeometry(2.5, 0.1, 8, 50), ringMat3);

    ring2.rotation.x = Math.PI / 2;
    ring3.rotation.y = Math.PI / 2;

    coreGroup.add(ring1);
    coreGroup.add(ring2);
    coreGroup.add(ring3);

    // 🔴 NÚCLEO (Esfera central)
    const coreGeo = new THREE.IcosahedronGeometry(0.8, 1);
    const coreMat = new THREE.MeshBasicMaterial({ color: 0xaa0000, wireframe: true });
    const coreSphere = new THREE.Mesh(coreGeo, coreMat);
    coreGroup.add(coreSphere);

    // Partículas (Polvo de estrellas rojo)
    const particlesGeo = new THREE.BufferGeometry();
    const particlesCount = 500;
    const posArray = new Float32Array(particlesCount * 3);
    for(let i = 0; i < particlesCount * 3; i++) {
        posArray[i] = (Math.random() - 0.5) * 20;
    }
    particlesGeo.setAttribute('position', new THREE.BufferAttribute(posArray, 3));
    const particlesMat = new THREE.PointsMaterial({ size: 0.02, color: 0xff0000 });
    const particlesMesh = new THREE.Points(particlesGeo, particlesMat);
    scene.add(particlesMesh);

    camera.position.z = 6;
    camera.position.y = 1;

    // Movimiento del ratón para Parallax
    let mouseX = 0;
    let mouseY = 0;
    const handleMouseMove = (event) => {
      mouseX = (event.clientX / window.innerWidth) * 2 - 1;
      mouseY = -(event.clientY / window.innerHeight) * 2 + 1;
    };
    window.addEventListener("mousemove", handleMouseMove);
    
    // Resize handler
    const handleResize = () => {
        camera.aspect = window.innerWidth / window.innerHeight;
        camera.updateProjectionMatrix();
        renderer.setSize(window.innerWidth, window.innerHeight);
    };
    window.addEventListener("resize", handleResize);

    // 🔥 ANIMACIÓN
    let frameId;
    let time = 0;

    const animate = () => {
      frameId = requestAnimationFrame(animate);
      time += 0.01;

      // Rotaciones
      ring1.rotation.z += 0.005;
      ring1.rotation.y += 0.002;
      
      ring2.rotation.x -= 0.008;
      ring2.rotation.z -= 0.003;
      
      ring3.rotation.y += 0.004;
      ring3.rotation.x += 0.001;

      coreSphere.rotation.y -= 0.01;
      coreSphere.rotation.z += 0.005;

      particlesMesh.rotation.y += 0.001;

      // Parallax de la cámara
      camera.position.x += (mouseX * 2 - camera.position.x) * 0.05;
      camera.position.y += (mouseY * 1 + 1 - camera.position.y) * 0.05;
      camera.lookAt(coreGroup.position);

      // Pulso del logo (UI HTML)
      if (logoRef.current) {
        const scale = 1 + Math.sin(time * 3) * 0.05;
        logoRef.current.style.transform = `scale(${scale})`;
        logoRef.current.style.filter = `drop-shadow(0 0 ${10 + Math.sin(time * 3) * 5}px rgba(255, 0, 0, 0.8))`;
      }

      renderer.render(scene, camera);
    };

    animate();

    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("resize", handleResize);
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

  async function handleLogin(e) {
    e.preventDefault();
    setLoading(true);
    try {
      const ok = await invoke("login", { user, pass });
      if (ok) {
        onSuccess();
      } else {
        alert("Credenciales incorrectas");
      }
    } catch (err) {
      alert("Error: " + err);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div
      style={{
        position: "relative",
        height: "100vh",
        background: "#050505",
        overflow: "hidden"
      }}
    >
      {/* 🔥 THREE BACKGROUND */}
      <div
        ref={mountRef}
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          width: "100%",
          height: "100%",
          zIndex: 0,
        }}
      />

      {/* 🖥️ PANEL UI (GLASSMORPHISM) */}
      <div
        style={{
          position: "relative",
          zIndex: 1,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          fontFamily: "'Inter', system-ui, sans-serif",
        }}
      >
        <div
          style={{
            padding: "40px 30px",
            background: "rgba(15, 10, 10, 0.4)",
            backdropFilter: "blur(16px)",
            WebkitBackdropFilter: "blur(16px)",
            border: "1px solid rgba(255, 50, 50, 0.15)",
            boxShadow: "0 8px 32px 0 rgba(255, 0, 0, 0.2)",
            borderRadius: "16px",
            width: "320px",
            display: "flex",
            flexDirection: "column",
            gap: "20px",
          }}
        >
          {/* 🔴 LOGO CON PULSO */}
          <div style={{ textAlign: "center" }}>
             <img
               ref={logoRef}
               src="/glefperu.png"
               alt="logo"
               style={{
                 width: "80px",
                 display: "block",
                 margin: "0 auto 10px",
                 transition: "transform 0.1s linear",
               }}
             />
             <h2 style={{ 
               margin: 0, 
               color: "#ffffff", 
               fontSize: "20px", 
               fontWeight: "600",
               letterSpacing: "2px" 
             }}>
               RFID SYSTEM
             </h2>
             <p style={{ margin: "5px 0 0 0", fontSize: "12px", color: "rgba(255, 255, 255, 0.5)" }}>
               Secure Access Terminal
             </p>
          </div>

          <form onSubmit={handleLogin} style={{ display: "flex", flexDirection: "column", gap: "15px" }}>
            <div style={inputContainerStyle}>
                <input
                  required
                  placeholder="USERNAME"
                  onChange={(e) => setUser(e.target.value)}
                  style={inputStyle}
                  className="modern-input"
                />
            </div>
            
            <div style={inputContainerStyle}>
                <input
                  required
                  type="password"
                  placeholder="PASSWORD"
                  onChange={(e) => setPass(e.target.value)}
                  style={inputStyle}
                  className="modern-input"
                />
            </div>

            <button 
                type="submit" 
                style={buttonStyle} 
                disabled={loading}
                onMouseOver={(e) => { e.currentTarget.style.boxShadow = "0 0 20px rgba(255, 0, 0, 0.6)"; e.currentTarget.style.transform = "translateY(-1px)"; }}
                onMouseOut={(e) => { e.currentTarget.style.boxShadow = "none"; e.currentTarget.style.transform = "translateY(0)"; }}
            >
              {loading ? "VERIFYING..." : "INITIALIZE"}
            </button>
          </form>
        </div>
      </div>
      
      <style>{`
        .modern-input:focus {
          outline: none;
          background: rgba(255, 255, 255, 0.1) !important;
          border-bottom: 2px solid #ff3333 !important;
        }
        .modern-input::placeholder {
          color: rgba(255, 255, 255, 0.3);
          letter-spacing: 1px;
          font-size: 12px;
        }
      `}</style>
    </div>
  );
}

const inputContainerStyle = {
    position: "relative",
    width: "100%",
};

const inputStyle = {
  width: "100%",
  boxSizing: "border-box",
  padding: "12px 15px",
  background: "rgba(0, 0, 0, 0.3)",
  border: "none",
  borderBottom: "2px solid rgba(255, 50, 50, 0.3)",
  color: "white",
  fontSize: "14px",
  fontFamily: "'Inter', monospace",
  transition: "all 0.3s ease",
  borderRadius: "4px 4px 0 0",
};

const buttonStyle = {
  width: "100%",
  padding: "14px",
  background: "linear-gradient(135deg, #cc0000 0%, #880000 100%)",
  border: "1px solid rgba(255, 50, 50, 0.5)",
  borderRadius: "8px",
  color: "white",
  cursor: "pointer",
  fontWeight: "bold",
  letterSpacing: "2px",
  fontSize: "14px",
  transition: "all 0.2s ease",
  marginTop: "10px",
};

export default Login;