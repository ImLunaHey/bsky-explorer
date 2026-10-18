// Recreate one service without touching its siblings or writing credentials to disk.
// Usage: node deploy/update-service.mjs api <image>
//        node deploy/update-service.mjs frontend <image>
// Collector updates additionally require --allow-ingester-restart.
import { execFileSync } from 'node:child_process';
import http from 'node:http';

const [service, image, confirmation] = process.argv.slice(2);
if (!['api', 'frontend', 'ingester'].includes(service) || !image) {
  throw Error('Usage: update-service.mjs api|frontend|ingester IMAGE [--allow-ingester-restart]');
}
if (service === 'ingester' && confirmation !== '--allow-ingester-restart') {
  throw Error('Refusing to interrupt collection without --allow-ingester-restart');
}
const inspect = name => JSON.parse(execFileSync('docker', ['inspect', name], { encoding: 'utf8' }))[0];
const name = `bsky-explorer-${service}-1`;
const old = inspect(name);
if (!old.State.Running) throw Error(`${name} is not running; inspect it before deployment`);
const imageId = inspect(image).Id;
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
const suffix = Date.now();
const stagingName = `${name}-prepared-${suffix}`;
const rollbackName = `${name}-previous-${suffix}`;

async function call(path, body) {
  return new Promise((resolve, reject) => {
    const request = http.request({ socketPath: '/var/run/docker.sock', path, method: 'POST', headers: { 'Content-Type': 'application/json' } }, response => {
      let data = '';
      response.on('data', chunk => data += chunk);
      response.on('end', () => response.statusCode < 300
        ? resolve(data ? JSON.parse(data) : {})
        : reject(Error(`Docker request failed (${response.statusCode}): ${data}`)));
    });
    request.on('error', reject);
    request.end(body === undefined ? undefined : JSON.stringify(body));
  });
}

const created = await call(`/containers/create?name=${stagingName}`, {
  Image: imageId, Env: old.Config.Env, Entrypoint: old.Config.Entrypoint,
  Cmd: old.Config.Cmd, User: old.Config.User, WorkingDir: old.Config.WorkingDir,
  ExposedPorts: old.Config.ExposedPorts,
  Labels: { ...old.Config.Labels, 'org.opencontainers.image.revision': revision },
  HostConfig: old.HostConfig,
  NetworkingConfig: { EndpointsConfig: Object.fromEntries(Object.keys(old.NetworkSettings.Networks).map(network => [network, { Aliases: [service] }])) },
});
let renamed = false;
try {
  await call(`/containers/${old.Id}/update`, { RestartPolicy: { Name: 'no' } });
  await call(`/containers/${old.Id}/stop?t=30`);
  await call(`/containers/${old.Id}/rename?name=${rollbackName}`);
  renamed = true;
  await call(`/containers/${created.Id}/rename?name=${name}`);
  await call(`/containers/${created.Id}/start`);
  let healthy = false;
  for (let attempt = 0; attempt < 60; attempt++) {
    const state = inspect(created.Id).State;
    if (state.Health?.Status === 'healthy') { healthy = true; break; }
    if (!state.Running) throw Error('Replacement exited before readiness');
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
  if (!healthy) throw Error('Replacement did not become healthy within 60 seconds');
  console.log(`${name} healthy; stopped rollback retained as ${rollbackName}. Other services were not restarted.`);
} catch (error) {
  await call(`/containers/${created.Id}/update`, { RestartPolicy: { Name: 'no' } }).catch(() => {});
  await call(`/containers/${created.Id}/stop?t=20`).catch(() => {});
  await call(`/containers/${created.Id}/rename?name=${stagingName}-failed`).catch(() => {});
  if (renamed) await call(`/containers/${old.Id}/rename?name=${name}`);
  await call(`/containers/${old.Id}/update`, { RestartPolicy: old.HostConfig.RestartPolicy });
  await call(`/containers/${old.Id}/start`);
  throw error;
}
