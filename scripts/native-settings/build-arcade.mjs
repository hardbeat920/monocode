// Bundle the exact React game engines for GPUI's bounded interpreter.
import {createRequire} from 'node:module';
import path from 'node:path';
const root=process.cwd();
const require=createRequire(path.join(process.env.MONOCODE_REFERENCE_ROOT||root,'package.json'));
const {build}=require('esbuild');
await build({stdin:{contents:`import {GRID_GAMES} from './src/features/terminal/arcade/gridGames';
import {PROJECT_MASCOTS} from './src/features/projects/model/projectMascots';
import * as R from './src/features/sessions/model/composerRunner';
let along=0,facing=1,previousWidth=0,last=0,wasBusy=false,exitAt=null,frozen=null,learned=false,stunAt=null,hitAlong=0,hitFacing=1,nextCoinAt=Infinity,coins=[];
export function runnerFrame(now,width,busy,obstacleJson,reduced,mascot){
 const obstacle=JSON.parse(obstacleJson),dt=Math.max(0,Math.min(80,now-last));last=now;const track=Math.max(0,width-20);
 if(previousWidth&&previousWidth!==width){along=R.scaleTrackX(along,Math.max(0,previousWidth-20),track);hitAlong=R.scaleTrackX(hitAlong,Math.max(0,previousWidth-20),track);for(const c of coins)c.x=R.scaleTrackX(c.x,previousWidth,width);}previousWidth=width;
 if(busy&&!wasBusy){exitAt=null;stunAt=null;learned=reduced;nextCoinAt=now+R.nextCoinDelay(true);coins=[];}
 if(!busy&&wasBusy){exitAt=now;frozen=R.poseAt(along,facing,width,null,[]);stunAt=null;for(const c of coins)if(c.collectedAt==null)c.collectedAt=now;}wasBusy=busy;
 if(!busy&&exitAt==null)return JSON.stringify({active:false,rows:[],coins:[],stars:[],x:0,y:0,facing:1});
 if(exitAt!=null){const t=reduced?1:Math.min(1,(now-exitAt)/R.EXIT_MS);return JSON.stringify({...frozen,y:R.exitJumpY(t),active:t<1,rows:PROJECT_MASCOTS.find(m=>m.name===mascot)?.rest||[],coins:[],stars:[]});}
 if(!reduced&&stunAt==null){const step=R.stepAlong(along,facing,dt,track);along=step.along;facing=step.facing;}
 if(stunAt!=null){along=R.recoilAlong(hitAlong,hitFacing,now-stunAt,track);facing=hitFacing;if(R.stunDone(now-stunAt)){learned=true;stunAt=null;}}
 const pose=R.poseAt(along,facing,width,learned?obstacle:null,stunAt==null?coins:[]);
 if(stunAt==null&&R.hitsChevron(pose.x,pose.y,pose.facing,obstacle,learned)){stunAt=now;hitAlong=along;hitFacing=facing;}
 if(!reduced&&stunAt==null&&!coins.some(c=>c.collectedAt==null)&&now>=nextCoinAt){const x=R.pickCoinX(width,pose.x,obstacle);if(x!=null)coins.push({x,height:R.COIN_HOVER,collectedAt:null});else nextCoinAt=now+2000;}
 const live=[];for(const coin of coins){if(stunAt==null&&coin.collectedAt==null&&R.coinCollected(pose,coin)){coin.collectedAt=now;nextCoinAt=now+R.nextCoinDelay(false);}const pop=coin.collectedAt==null?0:Math.min(1,(now-coin.collectedAt)/R.COLLECT_POP_MS);if(pop<1)live.push({x:coin.x,y:coin.height+(coin.collectedAt==null?Math.sin(now/180)*2:0)+R.COLLECT_POP_PX*pop,alpha:1-pop});}
 coins=coins.filter(c=>c.collectedAt==null||now-c.collectedAt<R.COLLECT_POP_MS||R.jumpHeight(pose.x,null,[c])>.5);
 const shake=stunAt==null?{x:0,y:0}:R.stunShake(now-stunAt);return JSON.stringify({...pose,x:pose.x+shake.x,y:pose.y-shake.y,active:true,rows:PROJECT_MASCOTS.find(m=>m.name===mascot)?.[!reduced&&Math.floor(now/180)%2?'talk':'rest']||[],coins:live,stars:stunAt==null?[]:R.stunStars(now-stunAt)});
}

const boards=GRID_GAMES.map(g=>g.create()); let lastIndex=0;
export function frame(cols,rows,dt,playing,index,mode,heading){
 const [dx,dy]=heading;
 const game=GRID_GAMES[index],board=boards[index];
 if(lastIndex!==index){boards[lastIndex].releaseControl();lastIndex=index;}
 board.resize(cols,rows);board.setMode(mode);
 if(playing&&!board.controlled())board.takeControl();else if(!playing&&board.controlled())board.releaseControl();
 if(dx||dy)board.steer(dx,dy);board.step(dt);
 const stamp=new Float32Array(cols*rows);board.stamp(stamp,cols,rows);
 const cells=[];for(let i=0;i<stamp.length;i++)if(stamp[i]>.02)cells.push([i,stamp[i]]);
 const sprites=board.sprites().map(s=>({...s,rows:PROJECT_MASCOTS.find(m=>m.name===s.mascot)?.[s.frame||'rest']}));
 return JSON.stringify({cells,sprites,pickup:board.logoPickup(),bubble:board.speechBubble(),score:board.score(),lives:board.lives(),dim:playing?1:game.idleDim,fade:board.fade()});
}`,resolveDir:root,loader:'ts'},bundle:true,format:'iife',globalName:'NativeArcade',platform:'neutral',target:'es2020',minify:true,outfile:'crates/monocode-gpui/assets/settings-arcade.js',plugins:[{name:'native-provider-ids',setup(build){build.onLoad({filter:/features\/sessions\/model\/session\.ts$/},()=>({contents:'export const HARNESSES=["claude","codex","cursor","grok","opencode","pi","omp","fx","hermes","antigravity","devin"];',loader:'js'}));}}]});
