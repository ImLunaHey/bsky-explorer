// Independent, restartable derived-data worker. Never touches the ingestion cursor.
import { execFileSync } from 'node:child_process';
import http from 'node:http';
const [image] = process.argv.slice(2);
if (!image) throw Error('Usage: node deploy/start-follow-resolution.mjs IMAGE');
const inspect = name => JSON.parse(execFileSync('docker', ['inspect', name], { encoding: 'utf8' }))[0];
const api = inspect('bsky-explorer-api-1');
const runtime = inspect(image);
const env = api.Config.Env.filter(v => /^CLICKHOUSE_/.test(v));
env.push('RUST_LOG=info');
const name = 'bsky-explorer-follow-resolution-1';
const request = (path, body) => new Promise((resolve,reject) => {
  const req = http.request({socketPath:'/var/run/docker.sock',path,method:'POST',headers:{'Content-Type':'application/json'}}, res => {
    let data='';res.on('data',chunk=>data+=chunk);res.on('end',()=>res.statusCode<300?resolve(data?JSON.parse(data):{}):reject(Error(`Docker request failed (${res.statusCode})`)));
  });req.on('error',reject);req.end(body===undefined?undefined:JSON.stringify(body));
});
const created = await request(`/containers/create?name=${name}`, {
  Image: runtime.Id, Entrypoint:['resolve_follows'], Env:env,
  Healthcheck:{Test:['CMD-SHELL','test -f /tmp/follow-ready && test $(($(date +%s) - $(cat /tmp/follow-ready))) -lt 180'],Interval:10000000000,Timeout:3000000000,Retries:3,StartPeriod:60000000000},
  Labels:{'com.docker.compose.project':'bsky-explorer','com.docker.compose.service':'follow-resolution','org.opencontainers.image.revision':execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim()},
  HostConfig:{RestartPolicy:{Name:'unless-stopped'},Memory:1073741824,NanoCpus:1000000000},
  NetworkingConfig:{EndpointsConfig:Object.fromEntries(Object.keys(api.NetworkSettings.Networks).map(network=>[network,{Aliases:['follow-resolution']}]))},
});
await request(`/containers/${created.Id}/start`);
console.log(`${name} started. Check Docker health and /api/follow-resolution for progress. No other services were restarted.`);
