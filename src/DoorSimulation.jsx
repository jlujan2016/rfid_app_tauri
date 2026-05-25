import React, { useState, useEffect, useRef } from 'react';
import { Canvas, useFrame } from '@react-three/fiber';
import { OrbitControls, Box, Plane, Text } from '@react-three/drei';
import * as THREE from 'three';
import './DoorSimulation.css';

const INITIAL_DOOR_BOXES = [
  { id: 'LAP-001', name: 'Laptop Trabajo', position: [-1, 0.2, -2], size: [0.8, 0.1, 0.6], color: '#3b82f6', canLeave: true, status: 'inside', entryTime: Date.now(), exitTime: null },
  { id: 'DOC-001', name: 'Doc. Confidencial', position: [1, 0.2, -3], size: [0.6, 0.2, 0.4], color: '#f59e0b', canLeave: false, status: 'inside', entryTime: Date.now(), exitTime: null },
  { id: 'PROY-001', name: 'Proyector 4K', position: [0, 0.5, -4], size: [0.8, 0.5, 0.8], color: '#8b5cf6', canLeave: false, status: 'inside', entryTime: Date.now(), exitTime: null },
  { id: 'TAB-001', name: 'Tablet Empresa', position: [2, 0.2, -2], size: [0.5, 0.1, 0.7], color: '#14b8a6', canLeave: true, status: 'inside', entryTime: Date.now(), exitTime: null },
];

const AntennaGate = ({ isAlarmActive }) => {
  const lightRef = useRef();

  useFrame(({ clock }) => {
    if (isAlarmActive && lightRef.current) {
      // Parpadeo rojo cuando hay alarma
      lightRef.current.color.setHex(Math.sin(clock.getElapsedTime() * 10) > 0 ? 0xff0000 : 0x440000);
    } else if (lightRef.current) {
      // Luz normal azulada
      lightRef.current.color.setHex(0x3b82f6);
    }
  });

  return (
    <group position={[0, 1.5, 0]}>
      {/* Marco Izquierdo */}
      <Box args={[0.3, 3, 0.5]} position={[-1.5, 0, 0]}>
        <meshStandardMaterial color="#1e293b" />
      </Box>
      {/* Marco Derecho */}
      <Box args={[0.3, 3, 0.5]} position={[1.5, 0, 0]}>
        <meshStandardMaterial color="#1e293b" />
      </Box>
      {/* Marco Superior */}
      <Box args={[3.3, 0.3, 0.5]} position={[0, 1.5, 0]}>
        <meshStandardMaterial color="#1e293b" />
      </Box>
      
      {/* Luces Indicadoras */}
      <Box args={[3.3, 0.1, 0.6]} position={[0, 1.5, 0]}>
        <meshStandardMaterial ref={lightRef} emissive="#000000" emissiveIntensity={2} color="#3b82f6" />
      </Box>
    </group>
  );
};

const OfficeScene = ({ isAlarmActive }) => {
  return (
    <group>
      {/* Floor */}
      <Plane args={[20, 20]} rotation={[-Math.PI / 2, 0, 0]} position={[0, 0, 0]} receiveShadow>
        <meshStandardMaterial color="#334155" />
      </Plane>

      {/* Wall Left */}
      <Box args={[8.5, 3, 0.2]} position={[-5.75, 1.5, 0]} receiveShadow>
        <meshStandardMaterial color="#475569" transparent opacity={0.8} />
      </Box>
      {/* Wall Right */}
      <Box args={[8.5, 3, 0.2]} position={[5.75, 1.5, 0]} receiveShadow>
        <meshStandardMaterial color="#475569" transparent opacity={0.8} />
      </Box>

      {/* Door Gate */}
      <AntennaGate isAlarmActive={isAlarmActive} />
      
      {/* Labels */}
      <Text position={[0, 0.1, -2]} rotation={[-Math.PI/2, 0, 0]} fontSize={0.5} color="#94a3b8">INTERIOR (Oficina)</Text>
      <Text position={[0, 0.1, 2]} rotation={[-Math.PI/2, 0, 0]} fontSize={0.5} color="#94a3b8">EXTERIOR</Text>
    </group>
  );
};

const DraggableItem = ({ box, onDrag, onDragEnd }) => {
  const meshRef = useRef();
  const [hovered, setHovered] = useState(false);
  const [dragging, setDragging] = useState(false);
  
  const planeRef = useRef(new THREE.Plane(new THREE.Vector3(0, 1, 0), -box.position[1]));
  const intersection = new THREE.Vector3();

  const handlePointerDown = (e) => {
    e.stopPropagation();
    setDragging(true);
    e.target.setPointerCapture(e.pointerId);
  };

  const handlePointerUp = (e) => {
    e.stopPropagation();
    setDragging(false);
    e.target.releasePointerCapture(e.pointerId);
    if (onDragEnd) onDragEnd(box.id, meshRef.current.position);
  };

  const handlePointerMove = (e) => {
    if (dragging) {
      e.stopPropagation();
      e.camera.updateMatrixWorld();
      e.ray.intersectPlane(planeRef.current, intersection);
      
      meshRef.current.position.x = intersection.x;
      meshRef.current.position.z = intersection.z;
      
      if (onDrag) onDrag(box.id, [intersection.x, box.position[1], intersection.z]);
    }
  };

  // Color depende del estado
  let currentColor = box.color;
  let emissiveColor = '#000000';
  let emissiveIntensity = 0;

  if (box.status === 'alarm' || box.status === 'alert') {
    currentColor = '#ef4444';
    emissiveColor = '#ef4444';
    emissiveIntensity = 0.5;
  }

  return (
    <Box
      ref={meshRef}
      args={box.size}
      position={box.position}
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerMove={handlePointerMove}
      onPointerOver={(e) => { e.stopPropagation(); setHovered(true); }}
      onPointerOut={(e) => { e.stopPropagation(); setHovered(false); }}
      castShadow
      receiveShadow
    >
      <meshStandardMaterial 
        color={currentColor} 
        emissive={emissiveColor}
        emissiveIntensity={emissiveIntensity}
        transparent
        opacity={hovered ? 0.8 : 1}
      />
      {hovered && !dragging && (
         <Text position={[0, box.size[1]/2 + 0.3, 0]} fontSize={0.2} color="white" anchorX="center" anchorY="middle">
           {box.name}
         </Text>
      )}
    </Box>
  );
};

export default function DoorSimulation() {
  const [boxes, setBoxes] = useState(INITIAL_DOOR_BOXES);
  const [orbitEnabled, setOrbitEnabled] = useState(true);
  const [logs, setLogs] = useState([{ id: 0, time: new Date(), msg: 'Sistema iniciado.', type: 'info' }]);
  const [simulatedTimeOffset, setSimulatedTimeOffset] = useState(0);
  const [passwordPrompt, setPasswordPrompt] = useState({ active: false, boxId: null });
  const [passwordInput, setPasswordInput] = useState('');
  const logContainerRef = useRef(null);

  useEffect(() => {
    if (logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [logs]);

  const addLog = (msg, type = 'info') => {
    setLogs(prev => [...prev, { id: Date.now(), time: new Date(), msg, type }].slice(-50));
  };

  const checkThreshold = (zPos) => {
    return zPos > 0 ? 'outside' : 'inside';
  };

  const handleBoxDragEnd = (id, newPos) => {
    setOrbitEnabled(true);
    setBoxes(prev => prev.map(b => {
      if (b.id === id) {
        const newLocation = checkThreshold(newPos.z);
        let newStatus = b.status;
        let newExitTime = b.exitTime;

        let newEntryTime = b.entryTime;

        if (newLocation === 'outside' && b.status === 'inside') {
          // Acaba de salir
          newExitTime = Date.now() + simulatedTimeOffset;
          if (!b.canLeave) {
            newStatus = 'alarm';
            addLog(`ALERTA: Equipo no autorizado salió (${b.name})`, 'alert');
          } else {
            newStatus = 'outside';
            addLog(`Registro: Equipo salió (${b.name})`, 'info');
          }
        } else if (newLocation === 'inside' && b.status !== 'inside') {
          // Acaba de entrar
          newStatus = 'inside';
          newEntryTime = Date.now() + simulatedTimeOffset;
          if (b.status === 'alarm') {
             addLog(`Alerta resuelta: Equipo devuelto (${b.name})`, 'info');
          } else {
             addLog(`Registro: Equipo devuelto (${b.name})`, 'info');
          }
        }

        return { 
          ...b, 
          position: [newPos.x, b.position[1], newPos.z],
          status: newStatus,
          exitTime: newExitTime,
          entryTime: newEntryTime
        };
      }
      return b;
    }));
  };

  const openPasswordPrompt = (id) => {
    setPasswordPrompt({ active: true, boxId: id });
    setPasswordInput('');
  };

  const handlePasswordSubmit = (e) => {
    e.preventDefault();
    if (passwordInput === "1234") {
      setBoxes(prev => prev.map(b => {
        if (b.id === passwordPrompt.boxId) {
          addLog(`Permiso modificado para: ${b.name}`, 'info');
          return { ...b, canLeave: !b.canLeave };
        }
        return b;
      }));
      setPasswordPrompt({ active: false, boxId: null });
      setPasswordInput('');
    } else {
      addLog('Error: Contraseña incorrecta.', 'alert');
      setPasswordInput('');
    }
  };

  const simulate24Hours = () => {
    const hours24Ms = 24 * 60 * 60 * 1000;
    setSimulatedTimeOffset(prev => prev + hours24Ms);
    
    setBoxes(prev => {
      let changed = false;
      const newBoxes = prev.map(b => {
        if (b.status === 'outside' && b.exitTime) {
          const currentTime = Date.now() + simulatedTimeOffset + hours24Ms;
          if (currentTime - b.exitTime >= hours24Ms) {
            changed = true;
            addLog(`ALERTA: Equipo no ha sido devuelto en 24h (${b.name})`, 'alert');
            return { ...b, status: 'alert' };
          }
        }
        return b;
      });
      return changed ? newBoxes : prev;
    });
    
    addLog('Simulación: Avanzaron 24 horas.', 'info');
  };

  const isAlarmActive = boxes.some(b => b.status === 'alarm' || b.status === 'alert');

  return (
    <div className="door-simulation-container">
      <div style={{ width: '100%', height: '100%' }}>
        <Canvas shadows camera={{ position: [5, 5, 8], fov: 45 }}>
          <ambientLight intensity={0.6} />
          <directionalLight position={[10, 15, 5]} intensity={1} castShadow shadow-mapSize={[1024, 1024]} />
          
          <OfficeScene isAlarmActive={isAlarmActive} />
          
          {boxes.map(box => (
            <DraggableItem 
              key={box.id} 
              box={box} 
              onDrag={() => setOrbitEnabled(false)}
              onDragEnd={handleBoxDragEnd} 
            />
          ))}
          <OrbitControls enabled={orbitEnabled} maxPolarAngle={Math.PI / 2 - 0.1} />
        </Canvas>
      </div>

      {/* Panel de UI sobre el Canvas */}
      <div className={`door-ui-panel ${isAlarmActive ? 'alarm-active' : ''}`}>
        <div className="door-ui-left">
          <h2 className="panel-title">Control de Puerta</h2>
          <button className="btn-simulate" onClick={simulate24Hours}>
            ⏩ Simular +24 Horas
          </button>

          {passwordPrompt.active && (
            <form onSubmit={handlePasswordSubmit} style={{ background: 'rgba(0,0,0,0.5)', padding: '10px', borderRadius: '6px', display: 'flex', gap: '5px', flexDirection: 'column' }}>
              <input 
                type="password" 
                placeholder="Contraseña..." 
                value={passwordInput}
                onChange={(e) => setPasswordInput(e.target.value)}
                autoFocus
                style={{ width: '100%', padding: '5px', borderRadius: '4px', border: '1px solid #3b82f6', background: '#1e293b', color: 'white' }}
              />
              <div style={{ display: 'flex', gap: '5px' }}>
                <button type="submit" style={{ flex: 1, background: '#10b981', color: 'white', border: 'none', padding: '5px 10px', borderRadius: '4px', cursor: 'pointer' }}>OK</button>
                <button type="button" onClick={() => setPasswordPrompt({ active: false, boxId: null })} style={{ background: '#ef4444', color: 'white', border: 'none', padding: '5px 10px', borderRadius: '4px', cursor: 'pointer' }}>X</button>
              </div>
            </form>
          )}
        </div>

        <div className="items-list">
          {boxes.map(b => (
            <div key={b.id} className="item-card">
              <div className="item-header">
                <span className="item-name">{b.name}</span>
                {b.status === 'inside' && <span className="badge badge-inside">En Oficina</span>}
                {b.status === 'outside' && <span className="badge badge-outside">Afuera</span>}
                {(b.status === 'alarm' || b.status === 'alert') && <span className="badge badge-alarm">ALERTA</span>}
              </div>
              <div className="time-label" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span>Permiso de salida: <strong>{b.canLeave ? 'SÍ' : 'NO'}</strong></span>
                <button onClick={() => openPasswordPrompt(b.id)} style={{ background: '#475569', color: '#fff', border: 'none', borderRadius: '4px', padding: '3px 8px', cursor: 'pointer', fontSize: '0.7rem' }}>
                  Cambiar
                </button>
              </div>
              <div className="time-label" style={{ fontSize: '0.65rem', marginTop: '2px' }}>
                {b.status !== 'inside' && b.exitTime ? `Salió: ${new Date(b.exitTime).toLocaleTimeString()}` : `Entró: ${new Date(b.entryTime).toLocaleTimeString()}`}
              </div>
            </div>
          ))}
        </div>

        <div className="event-log" ref={logContainerRef}>
          <h3 className="event-log-title" style={{ position: 'sticky', top: '-10px', background: 'rgba(15,23,42,0.9)', zIndex: 1, padding: '5px 0' }}>Registro de Eventos</h3>
          {logs.map(log => (
            <div key={log.id} className={`log-item ${log.type}`}>
              <span>{log.time.toLocaleTimeString()}</span>
              <span>{log.msg}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
