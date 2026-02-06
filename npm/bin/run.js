#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');
const os = require('os');
const fs = require('fs');

function getBinaryName() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = {
    darwin: 'darwin',
    linux: 'linux',
    win32: 'windows',
  };

  const archMap = {
    x64: 'x64',
    arm64: 'arm64',
  };

  const platformName = platformMap[platform];
  const archName = archMap[arch];

  if (!platformName || !archName) {
    console.error(`Unsupported platform: ${platform} ${arch}`);
    process.exit(1);
  }

  const ext = platform === 'win32' ? '.exe' : '';
  return `pdf-mcp-server-${platformName}-${archName}${ext}`;
}

function findBinary() {
  const binaryName = getBinaryName();

  // Check in binaries directory (installed via npm)
  const binariesDir = path.join(__dirname, '..', 'binaries');
  const binaryPath = path.join(binariesDir, binaryName);

  if (fs.existsSync(binaryPath)) {
    return binaryPath;
  }

  // Check if binary is in PATH
  const pathDirs = (process.env.PATH || '').split(path.delimiter);
  for (const dir of pathDirs) {
    const fullPath = path.join(dir, binaryName);
    if (fs.existsSync(fullPath)) {
      return fullPath;
    }
  }

  console.error(`Binary not found: ${binaryName}`);
  console.error('Please ensure the binary is installed correctly.');
  process.exit(1);
}

const binaryPath = findBinary();

// Make sure it's executable on Unix
if (os.platform() !== 'win32') {
  try {
    fs.chmodSync(binaryPath, 0o755);
  } catch (e) {
    // Ignore chmod errors
  }
}

// Spawn the binary with all arguments
const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
  env: process.env,
});

child.on('error', (err) => {
  console.error(`Failed to start binary: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code || 0);
  }
});
