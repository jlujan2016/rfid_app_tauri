import React, { useState, useEffect, useRef } from 'react';
import { Canvas, useFrame } from '@react-three/fiber';
import { OrbitControls, Box, Plane, Sphere, Text, Cylinder } from '@react-three/drei';
import * as THREE from 'three';
import './RfidSimulation.css';

// --- CONSTANTS ---
const ROOM_SIZE = 8;
const ROOM_HALF = ROOM_SIZE / 2;

const INITIAL_BOXES = [
  { id: 'CAJA-001', name: 'Zapatillas Nike', position: [-2, 1.5, -3], size: [0.8, 0.6, 0.8], color: '#f39c12', read: false, inRoom: true },
  { id: 'CAJA-002', name: 'Laptop Dell', position: [-2, 2.5, -3], size: [1.2, 0.3, 0.9], color: '#3498db', read: false, inRoom: true },
  { id: 'CAJA-003', name: 'Monitor LG', position: [0, 1.5, -3], size: [1.5, 1.2, 0.5], color: '#9b59b6', read: false, inRoom: true },
  { id: 'CAJA-004', name: 'Teclado Mecánico', position: [2, 1.5, -3], size: [1.0, 0.2, 0.4], color: '#e74c3c', read: false, inRoom: true },
  { id: 'CAJA-005', name: 'Mouse Inalámbrico', position: [2, 2.5, -3], size: [0.3, 0.2, 0.3], color: '#1abc9c', read: false, inRoom: true },
  { id: 'CAJA-006', name: 'Silla Gamer', position: [2, 0.5, 0], size: [1.2, 1.2, 1.2], color: '#e67e22', read: false, inRoom: true },
  { id: 'CAJA-007', name: 'Escritorio', position: [-2, 0.5, 2], size: [1.5, 1.0, 1.5], color: '#7f8c8d', read: false, inRoom: true },
];

const Antenna = ({ position, rotation }) => {
  return (
    <group position={position} rotation={rotation}>
      <Box args={[0.4, 0.4, 0.1]} position={[0, 0, 0]}>
        <meshStandardMaterial color="#ecf0f1" />
      </Box>
      <Cylinder args={[0.05, 0.05, 0.5]} position={[0, -0.2, -0.1]} rotation={[Math.PI / 2, 0, 0]}>
        <meshStandardMaterial color="#bdc3c7" />
      </Cylinder>
    </group>
  );
};

// Component for individual draggable box
const DraggableBoxBox = ({ box, onDrag, onDragEnd }) => {
  const meshRef = useRef();
  const [hovered, setHovered] = useState(false);
  const [dragging, setDragging] = useState(false);
  
  // Custom drag logic on a horizontal plane
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

  const targetColor = box.read ? '#2ecc71' : box.color;

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
        color={targetColor} 
        emissive={box.read ? '#2ecc71' : '#000000'}
        emissiveIntensity={box.read ? 0.4 : 0}
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

const Room = () => {
  return (
    <group>
      {/* Floor */}
      <Plane args={[ROOM_SIZE, ROOM_SIZE]} rotation={[-Math.PI / 2, 0, 0]} position={[0, 0, 0]} receiveShadow>
        <meshStandardMaterial color="#2c3e50" />
      </Plane>
      {/* Walls */}
      <Plane args={[ROOM_SIZE, 4]} position={[0, 2, -ROOM_HALF]} receiveShadow>
        <meshStandardMaterial color="#34495e" />
      </Plane>
      <Plane args={[ROOM_SIZE, 4]} rotation={[0, Math.PI / 2, 0]} position={[-ROOM_HALF, 2, 0]} receiveShadow>
        <meshStandardMaterial color="#34495e" />
      </Plane>
      
      {/* Shelves */}
      <Box args={[6, 0.2, 1.5]} position={[0, 1, -ROOM_HALF + 0.8]} receiveShadow castShadow>
        <meshStandardMaterial color="#7f8c8d" />
      </Box>
      <Box args={[6, 0.2, 1.5]} position={[0, 2, -ROOM_HALF + 0.8]} receiveShadow castShadow>
        <meshStandardMaterial color="#7f8c8d" />
      </Box>

      {/* Reader Hub */}
      <Box args={[0.5, 0.2, 0.5]} position={[0, 0.1, 0]}>
        <meshStandardMaterial color="#000" />
        <Text position={[0, 0.15, 0]} rotation={[-Math.PI/2, 0, 0]} fontSize={0.1} color="white">
          UR4 Reader
        </Text>
      </Box>

      {/* Antennas in 3 corners */}
      <Antenna position={[-ROOM_HALF + 0.5, 3, -ROOM_HALF + 0.5]} rotation={[Math.PI/4, Math.PI/4, 0]} />
      <Antenna position={[ROOM_HALF - 0.5, 3, -ROOM_HALF + 0.5]} rotation={[Math.PI/4, -Math.PI/4, 0]} />
      <Antenna position={[-ROOM_HALF + 0.5, 3, ROOM_HALF - 0.5]} rotation={[Math.PI/4, 3*Math.PI/4, 0]} />
    </group>
  );
};

function RfidSimulation() {
  const [boxes, setBoxes] = useState(INITIAL_BOXES);
  const [isReading, setIsReading] = useState(false);
  const [orbitEnabled, setOrbitEnabled] = useState(true);

  // Determine if a position is inside the room
  const checkInRoom = (pos) => {
    return (
      pos[0] > -ROOM_HALF && pos[0] < ROOM_HALF &&
      pos[2] > -ROOM_HALF && pos[2] < ROOM_HALF
    );
  };

  const handleBoxDrag = (id, newPos) => {
    // Disable orbit controls while dragging
    if (orbitEnabled) setOrbitEnabled(false);
  };

  const handleBoxDragEnd = (id, newPos) => {
    setOrbitEnabled(true);
    setBoxes(prev => prev.map(b => {
      if (b.id === id) {
        return { 
          ...b, 
          position: [newPos.x, b.position[1], newPos.z],
          inRoom: checkInRoom([newPos.x, b.position[1], newPos.z]),
          // If moved outside, maybe reset read status so it's clearly missing
          read: checkInRoom([newPos.x, b.position[1], newPos.z]) ? b.read : false 
        };
      }
      return b;
    }));
  };

  useEffect(() => {
    let interval;
    if (isReading) {
      interval = setInterval(() => {
        setBoxes(prevBoxes => {
          const unreadBoxes = prevBoxes.filter(b => b.inRoom && !b.read);
          if (unreadBoxes.length === 0) {
            return prevBoxes; // Sigue activo escaneando
          }
          
          // Escoge una caja aleatoria para leer
          const boxToRead = unreadBoxes[Math.floor(Math.random() * unreadBoxes.length)];
          
          return prevBoxes.map(b => 
            b.id === boxToRead.id ? { ...b, read: true } : b
          );
        });
      }, 800); // intenta leer cada 800ms
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [isReading]);

  const startSimulation = () => {
    setIsReading(true);
  };

  const stopSimulation = () => {
    setIsReading(false);
  };

  const totalItems = boxes.length;
  const readItems = boxes.filter(b => b.read).length;
  const missingItems = boxes.filter(b => !b.inRoom).length;

  return (
    <div className="rfid-simulation-container">
      {/* 3D Canvas Area */}
      <div className="canvas-wrapper">
        <Canvas shadows camera={{ position: [0, 5, 8], fov: 50 }}>
          <ambientLight intensity={0.5} />
          <directionalLight position={[10, 10, 5]} intensity={1} castShadow shadow-mapSize={[1024, 1024]} />
          <Room />
          {boxes.map(box => (
            <DraggableBoxBox 
              key={box.id} 
              box={box} 
              onDrag={handleBoxDrag}
              onDragEnd={handleBoxDragEnd} 
            />
          ))}
          <OrbitControls 
            enabled={orbitEnabled}
            maxPolarAngle={Math.PI / 2 - 0.05} // don't go below floor
            minDistance={2}
            maxDistance={15}
          />
        </Canvas>
      </div>

      {/* UI Overlay Panel */}
      <div className="ui-panel">
        <h2 className="panel-title">Lectura RFID - Sala de Inventario</h2>
        
        <div className="simulation-controls">
          {!isReading ? (
            <button className="btn-start" onClick={startSimulation}>
              ▶ Comenzar Lectura
            </button>
          ) : (
            <button className="btn-stop" onClick={stopSimulation}>
              ⏹ Detener Lectura
            </button>
          )}
        </div>

        <div className="stats-mini">
          <div className="stat-item">
            <span>Total:</span> <strong>{totalItems}</strong>
          </div>
          <div className="stat-item text-green">
            <span>Leídos:</span> <strong>{readItems}</strong>
          </div>
          <div className="stat-item text-red">
            <span>Fuera:</span> <strong>{missingItems}</strong>
          </div>
        </div>

        <div className="product-list">
          <h3>Productos</h3>
          <ul className="list-container">
            {boxes.map(box => (
              <li key={box.id} className={`list-item ${box.read ? 'read' : ''} ${!box.inRoom ? 'missing' : ''}`}>
                <div className="item-info">
                  <span className="item-id">{box.id}</span>
                  <span className="item-name">{box.name}</span>
                </div>
                <div className="item-status">
                  {!box.inRoom 
                    ? <span className="badge badge-red">Fuera</span>
                    : box.read 
                      ? <span className="badge badge-green">Leído</span>
                      : <span className="badge badge-gray">Pendiente</span>
                  }
                </div>
              </li>
            ))}
          </ul>
        </div>
        
        <div className="instructions">
          <p>ℹ️ <strong>Instrucciones:</strong></p>
          <ul>
             <li>Arrastra el ratón para rotar la cámara.</li>
             <li>Haz clic y arrastra una caja para moverla dentro o fuera de la habitación.</li>
             <li>Presiona "Comenzar Lectura" para simular el funcionamiento de las antenas RFID.</li>
          </ul>
        </div>
      </div>
    </div>
  );
}

export default RfidSimulation;
