// One-time split migration. First deploy this same runtime image to the
// ingester (removes its embedded worker), then run this helper, then update API.
// No secrets are printed or written to temporary files; no data is copied.
import { execFileSync } from 'node:child_process';
import http from 'node:http';
const [image] = process.argv.slice(2);
if (!image) throw Error('Usage: node deploy/start-identity-service.mjs IMAGE');
const inspect = name => JSON.parse(execFileSync('docker', ['inspect', name], { encoding: 'utf8' }))[0];
const ingester = inspect('bsky-explorer-ingester-1');
const runtime = inspect(image);
if (ingester.Image !== runtime.Id || !ingester.State.Running) throw Error('Deploy this runtime to the ingester first to remove its embedded identity worker.');
const archive = ingester.Mounts.find(m => m.Destination === '/data/archive' && m.Type === 'bind');
if (!archive || !archive.Source.startsWith('/mnt/storage/')) throw Error('Expected explicit archive bind under /mnt/storage; inspect deployment manually.');
const env = ingester.Config.Env.filter(v => /^(CLICKHOUSE_|IDENTITY_)/.test(v) && !/^(IDENTITY_SERVICE_URL|IDENTITY_QUEUE_DIR|IDENTITY_REQUESTS_PER_SECOND)=/.test(v));
env.push('SERVICE_ROLE=identity', 'IDENTITY_QUEUE_DIR=/data/identity', 'IDENTITY_REQUESTS_PER_SECOND=2', 'LISTEN_ADDR=0.0.0.0:3000', 'RUST_LOG=bsky_observatory=info');
const name = 'bsky-explorer-identity-1';
const request = (path, body) => new Promise((resolve,reject) => {
  const req = http.request({socketPath:'/var/run/docker.sock',path,method:'POST',headers:{'Content-Type':'application/json'}}, res => {
    let data='';res.on('data',chunk=>data+=chunk);res.on('end',()=>res.statusCode<300?resolve(data?JSON.parse(data):{}):reject(Error(`Docker request failed (${res.statusCode})`)));
  });req.on('error',reject);req.end(body===undefined?undefined:JSON.stringify(body));
});
const created = await request(`/containers/create?name=${name}`, {
  Image: runtime.Id, Entrypoint:['identity_service'], Env:env, ExposedPorts:{'3000/tcp':{}},
  Labels:{'com.docker.compose.project':'bsky-explorer','com.docker.compose.service':'identity','org.opencontainers.image.revision':execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim()},
  HostConfig:{Binds:[`${archive.Source}/identity-queue:/data/identity`],RestartPolicy:{Name:'unless-stopped'},Memory:1073741824,NanoCpus:1000000000},
  NetworkingConfig:{EndpointsConfig:Object.fromEntries(Object.keys(ingester.NetworkSettings.Networks).map(network=>[network,{Aliases:['identity']}]))},
});
await request(`/containers/${created.Id}/start`);
for (let i=0;i<60;i++) {
  const state=inspect(created.Id).State;
  if (state.Health?.Status==='healthy') { console.log(`${name} healthy; existing queue preserved; no host port published.`);process.exit(0); }
  if (!state.Running) throw Error('Identity service exited; inspect its logs. API has not been switched.');
  await new Promise(resolve=>setTimeout(resolve,1000));
}
throw Error('Identity service readiness timed out. API has not been switched.');
