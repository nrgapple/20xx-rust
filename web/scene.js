// Browser presentation only. Mesh/texture decoding and asset preparation are Rust.
// Rust advances original animation streams and skeletons; combat is not yet connected.
const $ = id => document.getElementById(id);
const canvas = $("scene");
const gl = canvas.getContext("webgl2", { alpha: false, antialias: true });
const assets = new Map();
let engine, previousTime, accumulated=0;
const decoder=new TextDecoder();
function check(status){if(status!==0)throw new Error(decoder.decode(new Uint8Array(engine.memory.buffer,engine.output_ptr(),engine.output_len())));}
function loadNative(slot,meta,bytes){
  const mp=engine.alloc(meta.byteLength),bp=engine.alloc(bytes.byteLength);
  try {new Uint8Array(engine.memory.buffer,mp,meta.byteLength).set(new Uint8Array(meta));new Uint8Array(engine.memory.buffer,bp,bytes.byteLength).set(new Uint8Array(bytes));check(engine.scene_load(slot,mp,meta.byteLength,bp,bytes.byteLength));}
  finally {engine.dealloc(mp,meta.byteLength);engine.dealloc(bp,bytes.byteLength);}
}
let program, uniforms, mode = "stage", yaw = 0.16, pitch = 0.14, zoom = 1;
const names = { fox: "Fox", mario: "Mario", marth: "Marth", falcon: "Captain Falcon" };
function error(message) {
  $("scene-status").textContent = message;
  $("loading").textContent = message;
  $("loading").classList.add("failed");
  $("loading").hidden = false;
}
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
    layout(location=3) in vec4 boneIndex;
    layout(location=4) in vec4 weight;
    uniform sampler2D bones;
    uniform mat4 viewProjection; uniform vec3 translation;
    vec3 skin(vec4 v){vec3 result=vec3(0.0);for(int i=0;i<4;i++){int b=int(boneIndex[i]);result+=weight[i]*vec3(dot(texelFetch(bones,ivec2(0,b),0),v),dot(texelFetch(bones,ivec2(1,b),0),v),dot(texelFetch(bones,ivec2(2,b),0),v));}return result;}
    out vec3 n; out vec2 texcoord;
    void main(){gl_Position=viewProjection*vec4(skin(vec4(position,1.0))+translation,1.0);n=skin(vec4(normal,0.0));texcoord=uv;}`);
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
  uniforms=Object.fromEntries(["viewProjection","translation","tex","color","textured","bones"].map(k=>[k,gl.getUniformLocation(program,k)]));
  gl.useProgram(program);gl.uniform1i(uniforms.tex,0);gl.uniform1i(uniforms.bones,1);gl.enable(gl.DEPTH_TEST);
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
async function loadAsset(id, entry, slot) {
  const meta=await verifiedBytes(`content/${entry.metadata}`,entry.metadata_sha256);
  const metadata=JSON.parse(decoder.decode(meta));
  if(metadata.version!==2)throw new Error("Unsupported scene format.");
  const bytes=await verifiedBytes(`content/${entry.binary}`,entry.binary_sha256);
  loadNative(slot,meta,bytes);
  const clips=metadata.animation?.clips??[];
  const idle=clips.findIndex(c=>c.name==="Wait1");
  if(idle>=0)check(engine.scene_play(slot,idle));
  const boneTexture=gl.createTexture();gl.activeTexture(gl.TEXTURE1);gl.bindTexture(gl.TEXTURE_2D,boneTexture);
  gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);
  gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA32F,3,metadata.joints*2,0,gl.RGBA,gl.FLOAT,new Float32Array(engine.memory.buffer,engine.scene_palette_ptr(slot),metadata.joints*24));
  gl.activeTexture(gl.TEXTURE0);
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
    gl.bufferData(gl.ARRAY_BUFFER,new Uint8Array(bytes,m.skin_offset,m.count*64),gl.STATIC_DRAW);
    [3,3,2,4,4].forEach((size,index)=>{gl.enableVertexAttribArray(index);gl.vertexAttribPointer(index,size,gl.FLOAT,false,64,[0,12,24,32,48][index]);});
    return {...m,vao,jointIndex:metadata.skeleton.findIndex(j=>j.offset===m.joint)};
  });
  assets.set(id,{...metadata,textures,meshes,slot,boneTexture,idle,frame:0,clips});
}
function normalize(v){const n=Math.hypot(...v);return v.map(x=>x/n);}
function cross(a,b){return[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];}
function dot(a,b){return a.reduce((sum,x,i)=>sum+x*b[i],0);}
function multiply(a,b){const result=new Float32Array(16);for(let c=0;c<4;c++)for(let r=0;r<4;r++)for(let k=0;k<4;k++)result[c*4+r]+=a[k*4+r]*b[c*4+k];return result;}
function camera(aspect){
  const stage=mode==="stage", target=[0,stage?5:9,0], distance=(stage?165:34)*zoom*Math.max(1,1.4/aspect);
  const eye=[Math.sin(yaw)*Math.cos(pitch)*distance,target[1]+Math.sin(pitch)*distance,Math.cos(yaw)*Math.cos(pitch)*distance];
  const z=normalize(eye.map((x,i)=>x-target[i])),x=normalize(cross([0,1,0],z)),y=cross(z,x);
  const view=[x[0],y[0],z[0],0,x[1],y[1],z[1],0,x[2],y[2],z[2],0,-dot(x,eye),-dot(y,eye),-dot(z,eye),1];
  const near=.1,far=5000,f=1/Math.tan(Math.PI/7);
  return multiply([f/aspect,0,0,0,0,f,0,0,0,0,(far+near)/(near-far),-1,0,0,2*far*near/(near-far),0],view);
}
function drawAsset(id,translation){
  const asset=assets.get(id);if(!asset)return;
  gl.activeTexture(gl.TEXTURE1);gl.bindTexture(gl.TEXTURE_2D,asset.boneTexture);
  gl.activeTexture(gl.TEXTURE0);
  gl.uniform3fv(uniforms.translation,translation);
  for(const mesh of asset.meshes){
    // Static ground geometry only: stage sky/particles need their animation and
    // TEV passes; displaying their bind-pose geometry hides the actual platform.
    if(id==="final-destination" && ![1,2,3].includes(mesh.root))continue;
    if(engine.scene_joint_hidden(asset.slot,mesh.jointIndex))continue;
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
  engine=(await WebAssembly.instantiateStreaming(fetch("melee_web.wasm"),{})).instance.exports;
  const response=await fetch("content/manifest.json");
  if(!response.ok)throw new Error("The original-asset content pack is not installed on this build.");
  const manifest=await response.json();
  await Promise.all(Object.keys(manifest.assets).map(async (id,slot)=>{await loadAsset(id,manifest.assets[id],slot);draw();}));
  $("scene-status").textContent="Scene loaded";
  $("loading").hidden=true;
  $("scene-view").disabled=false;$("scene-reset").disabled=false;
  requestAnimationFrame(animate);
}
function animate(now){
  if(document.hidden){previousTime=undefined;accumulated=0;requestAnimationFrame(animate);return;}
  if(previousTime!==undefined)accumulated+=Math.min(now-previousTime,100);
  previousTime=now;
  try {
    let changed=false;
    while(accumulated>=1000/60){
      for(const asset of assets.values())if(asset.idle>=0){
        // The display loops the selected idle clip; fighter action transitions
        // and random idle variants will be owned by the match simulation.
        if(++asset.frame>=asset.clips[asset.idle].end){check(engine.scene_play(asset.slot,asset.idle));asset.frame=0;}
        else check(engine.scene_advance(asset.slot));
      }
      accumulated-=1000/60;changed=true;
    }
    if(changed){
      for(const asset of assets.values())if(asset.idle>=0){
        gl.activeTexture(gl.TEXTURE1);gl.bindTexture(gl.TEXTURE_2D,asset.boneTexture);
        gl.texSubImage2D(gl.TEXTURE_2D,0,0,0,3,asset.joints*2,gl.RGBA,gl.FLOAT,new Float32Array(engine.memory.buffer,engine.scene_palette_ptr(asset.slot),asset.joints*24));
      }
      draw();
    }
    requestAnimationFrame(animate);
  }catch(e){error(e.message);}
}
main().catch(e=>error(e.message));
