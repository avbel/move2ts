#!/usr/bin/env node

const { spawnSync } = require('child_process');
const path = require('path');

const platformMap = {
  'linux-x64': '@move2ts/linux-x64',
  'linux-arm64': '@move2ts/linux-arm64',
  'darwin-x64': '@move2ts/darwin-x64',
  'darwin-arm64': '@move2ts/darwin-arm64',
  'win32-x64': '@move2ts/win32-x64',
};

const key = `${process.platform}-${process.arch}`;
const pkg = platformMap[key];

if (!pkg) {
  console.error(`Unsupported platform: ${key}`);
  console.error(`move2ts supports: ${Object.keys(platformMap).join(', ')}`);
  process.exit(1);
}

let binaryPath;
try {
  const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
  const binaryName = process.platform === 'win32' ? 'move2ts.exe' : 'move2ts';
  binaryPath = path.join(pkgDir, binaryName);
} catch {
  console.error(`Could not find package ${pkg}.`);
  console.error('Make sure the correct platform-specific package is installed.');
  console.error(`Try: npm install ${pkg}`);
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
});

if (result.error) {
  console.error(`Failed to execute move2ts: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
