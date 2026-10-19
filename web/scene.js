// Browser presentation only. Mesh/texture decoding and asset preparation are Rust.
// This viewer does not advance gameplay, collision or animation state.
const $ = id => document.getElementById(id);
const canvas = $("scene");
const gl = canvas.getContext("webgl2", { alpha: false, antialias: true });
const assets = new Map();
let program, uniforms, mode = "stage", yaw = 0.16, pitch = 0.14, zoom = 1;
const names = { fox: "Fox", mario: "Mario", marth: "Marth", falcon: "Captain Falcon" };
function error(message) { $("scene-status").textContent = message; }
function shader(type, source) {
  const shader = gl.createShader(type); gl.shaderSource(shader, source); gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
  return shader;
}
function setup() {
  if (!gl) throw new Error("This browser could not create a WebGL 2 context.");
  const vertex = shader(gl.VERTEX_SHADER, `#version 300 es
    precision highp float;
    layout(location=0) in vec3 position;
    layout(location=1) in vec3 normal;
    layout(location=2) in vec2 uv;
    uniform mat4 viewProjection; uniform vec3 translation;
    out vec3 n; out vec2 texcoord;
    void main(){gl_Position=viewProjection*vec4(position+translation,1.0);n=normal;texcoord=uv;}`);
  const fragment = shader(gl.FRAGMENT_SHADER, `#version 300 es
    precision highp float;
    in vec3 n; in vec2 texcoord;
    uniform sampler2D tex; uniform vec4 color; uniform bool textured;
    out vec4 outputColor;
    void main(){vec4 c=color*(textured?texture(tex,texcoord):vec4(1.0));
      if(c.a<0.1)discard;
      float light=0.65+0.35*abs(dot(normalize(n),normalize(vec3(0.3,0.8,1.0))));
      outputColor=vec4(c.rgb*light,c.a);}`);
  program=gl.createProgram();gl.attachShader(program,vertex);gl.attachShader(program,fragment);gl.linkProgram(program);
  if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw new Error(gl.getProgramInfoLog(program));
  uniforms=Object.fromEntries(["viewProjection","translation","tex","color","textured"].map(k=>[k,gl.getUniformLocation(program,k)]));
  gl.useProgram(program);gl.uniform1i(uniforms.tex,0);gl.enable(gl.DEPTH_TEST);
  gl.enable(gl.BLEND);gl.blendFunc(gl.SRC_ALPHA,gl.ONE_MINUS_SRC_ALPHA);
}
async function verifiedBytes(url, hash) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Content unavailable (${response.status}).`);
  const bytes = await response.arrayBuffer();
  const digest = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map(b => b.toString(16).padStart(2,"0")).join("");
  if(digest !== hash)throw new Error("Content checksum mismatch. Reload to fetch the current content pack.");
  return bytes;
}
async function loadAsset(id, entry) {
  const metadata = JSON.parse(new TextDecoder().decode(await verifiedBytes(`content/${entry.metadata}`, entry.metadata_sha256)));
  if(metadata.version!==1)throw new Error("Unsupported scene format.");
  const bytes=await verifiedBytes(`content/${entry.binary}`,entry.binary_sha256);
  const textures=metadata.textures.map(t=>{
    const tex=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,tex);
    gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,t.width,t.height,0,gl.RGBA,gl.UNSIGNED_BYTE,new Uint8Array(bytes,t.offset,t.width*t.height*4));
    gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.LINEAR);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_S,gl.REPEAT);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_T,gl.REPEAT);
    return tex;
  });
  const meshes=metadata.meshes.map(m=>{
    const vao=gl.createVertexArray();gl.bindVertexArray(vao);
    const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
    gl.bufferData(gl.ARRAY_BUFFER,new Uint8Array(bytes,m.offset,m.count*32),gl.STATIC_DRAW);
    [3,3,2].forEach((size,index)=>{gl.enableVertexAttribArray(index);gl.vertexAttribPointer(index,size,gl.FLOAT,false,32,[0,12,24][index]);});
    return {...m,vao};
  });
  assets.set(id,{...metadata,textures,meshes});
}
function normalize(v){const n=Math.hypot(...v);return v.map(x=>x/n);}
function cross(a,b){return[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];}
function dot(a,b){return a.reduce((sum,x,i)=>sum+x*b[i],0);}
function multiply(a,b){const result=new Float32Array(16);for(let c=0;c<4;c++)for(let r=0;r<4;r++)for(let k=0;k<4;k++)result[c*4+r]+=a[k*4+r]*b[c*4+k];return result;}
function camera(aspect){
  const stage=mode==="stage", target=[0,stage?5:9,0], distance=(stage?165:34)*zoom;
  const eye=[Math.sin(yaw)*Math.cos(pitch)*distance,target[1]+Math.sin(pitch)*distance,Math.cos(yaw)*Math.cos(pitch)*distance];
  const z=normalize(eye.map((x,i)=>x-target[i])),x=normalize(cross([0,1,0],z)),y=cross(z,x);
  const view=[x[0],y[0],z[0],0,x[1],y[1],z[1],0,x[2],y[2],z[2],0,-dot(x,eye),-dot(y,eye),-dot(z,eye),1];
  const near=.1,far=5000,f=1/Math.tan(Math.PI/7);
  return multiply([f/aspect,0,0,0,0,f,0,0,0,0,(far+near)/(near-far),-1,0,0,2*far*near/(near-far),0],view);
}
function drawAsset(id,translation){
  const asset=assets.get(id);if(!asset)return;
  gl.uniform3fv(uniforms.translation,translation);
  for(const mesh of asset.meshes){
    // Static ground geometry only: stage sky/particles need their animation and
    // TEV passes; displaying their bind-pose geometry hides the actual platform.
    if(id==="final-destination" && ![1,2,3].includes(mesh.root))continue;
    gl.bindVertexArray(mesh.vao);gl.uniform4fv(uniforms.color,mesh.color);
    gl.uniform1i(uniforms.textured,mesh.texture!==null);
    if(mesh.texture!==null)gl.bindTexture(gl.TEXTURE_2D,asset.textures[mesh.texture]);
    gl.drawArrays(gl.TRIANGLES,0,mesh.count);
  }
}
function draw(){
  if(!program||gl.isContextLost())return;
  const ratio=Math.min(devicePixelRatio||1,2),width=Math.round(canvas.clientWidth*ratio),height=Math.round(canvas.clientHeight*ratio);
  if(canvas.width!==width||canvas.height!==height){canvas.width=width;canvas.height=height;}
  gl.viewport(0,0,width,height);gl.clearColor(.035,.045,.075,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
  gl.useProgram(program);gl.uniformMatrix4fv(uniforms.viewProjection,false,camera(width/height));
  if(mode==="stage"){
    drawAsset("final-destination",[0,0,0]);
    Object.keys(names).forEach((id,i)=>drawAsset(id,[-54+i*36,0,0]));
  }else drawAsset(mode,[0,0,0]);
}
let drag;
canvas.addEventListener("pointerdown",e=>{drag=[e.clientX,e.clientY];canvas.setPointerCapture(e.pointerId);});
canvas.addEventListener("pointermove",e=>{if(!drag)return;yaw+=(e.clientX-drag[0])*.008;pitch=Math.max(-.7,Math.min(.9,pitch+(e.clientY-drag[1])*.006));drag=[e.clientX,e.clientY];draw();});
for(const event of ["pointerup","pointercancel","lostpointercapture"])canvas.addEventListener(event,()=>{drag=null;});
canvas.addEventListener("wheel",e=>{e.preventDefault();zoom=Math.max(.5,Math.min(2.5,zoom*Math.exp(e.deltaY*.001)));draw();},{passive:false});
canvas.addEventListener("keydown",e=>{if(["ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].includes(e.key)){e.preventDefault();yaw+=e.key==="ArrowLeft"?-.1:e.key==="ArrowRight"?.1:0;pitch=Math.max(-.7,Math.min(.9,pitch+(e.key==="ArrowUp"?.1:e.key==="ArrowDown"?-.1:0)));draw();}});
$("scene-view").addEventListener("change",e=>{mode=e.target.value;yaw=mode==="stage"?.16:0;pitch=.14;zoom=1;draw();});
$("scene-reset").addEventListener("click",()=>{yaw=.16;pitch=.14;zoom=1;draw();});
new ResizeObserver(draw).observe(canvas);
canvas.addEventListener("webglcontextlost",e=>{e.preventDefault();error("Graphics context lost. Reload to restore the scene.");});
async function main(){
  setup();draw();
  const response=await fetch("content/manifest.json");
  if(!response.ok)throw new Error("The original-asset content pack is not installed on this build.");
  const manifest=await response.json();
  await Promise.all(Object.keys(manifest.assets).map(async id=>{await loadAsset(id,manifest.assets[id]);draw();}));
  const tris=[...assets.values()].reduce((sum,a)=>sum+a.meshes.reduce((n,m)=>n+m.count/3,0),0);
  error(`Original model data loaded · ${tris.toLocaleString()} triangles · Rust-converted geometry and textures. Static archive poses; animation and exact material effects are not implemented.`);
  $("scene-view").disabled=false;$("scene-reset").disabled=false;
}
main().catch(e=>error(e.message));
