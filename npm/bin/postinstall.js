#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const os = require('os');

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
    console.warn(`⚠️  Unsupported platform: ${platform} ${arch}`);
    console.warn('   You may need to build from source or download manually.');
    return null;
  }

  const ext = platform === 'win32' ? '.exe' : '';
  return `pdf-mcp-server-${platformName}-${archName}${ext}`;
}

function extractBinary() {
  const binaryName = getBinaryName();
  if (!binaryName) return;

  const binariesDir = path.join(__dirname, '..', 'binaries');
  const archiveDir = path.join(binariesDir, binaryName.replace(/\.exe$/, ''));

  // Check for tar.gz or zip archive
  const tarPath = path.join(archiveDir, `${binaryName}.tar.gz`);
  const zipPath = path.join(archiveDir, `${binaryName}.zip`);

  if (fs.existsSync(tarPath)) {
    const { execSync } = require('child_process');
    execSync(`tar -xzf "${tarPath}" -C "${binariesDir}"`, { stdio: 'inherit' });
    console.log(`✅ Extracted ${binaryName}`);
  } else if (fs.existsSync(zipPath)) {
    const { execSync } = require('child_process');
    execSync(`unzip -o "${zipPath}" -d "${binariesDir}"`, { stdio: 'inherit' });
    console.log(`✅ Extracted ${binaryName}`);
  } else {
    // Binary might already be extracted or doesn't need extraction
    const binaryPath = path.join(binariesDir, binaryName);
    if (fs.existsSync(binaryPath)) {
      console.log(`✅ Binary found: ${binaryName}`);
      // Ensure executable on Unix
      if (os.platform() !== 'win32') {
        fs.chmodSync(binaryPath, 0o755);
      }
    } else {
      console.warn(`⚠️  Binary not found: ${binaryName}`);
      console.warn('   Please download from: https://github.com/paradyno/pdf-mcp-server/releases');
    }
  }
}

// Run extraction
extractBinary();

console.log('');
console.log('PDF MCP Server installed successfully!');
console.log('');
console.log('To use with Claude Desktop, add to your config:');
console.log('');
console.log('  {');
console.log('    "mcpServers": {');
console.log('      "pdf": {');
console.log('        "command": "npx",');
console.log('        "args": ["@paradyno/pdf-mcp-server"]');
console.log('      }');
console.log('    }');
console.log('  }');
console.log('');
