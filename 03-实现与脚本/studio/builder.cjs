const path = require('node:path');
module.exports = {
  appId: 'cn.wheelcombat.studio',
  productName: 'Wheel Combat Studio',
  directories: {output: path.resolve(__dirname, '../../06-交付物/demo-v0.3.5')},
  files: ['src/**', 'ui/**', 'package.json'],
  asar: true,
  npmRebuild: false,
  electronDist: path.resolve(__dirname, 'node_modules/electron/dist'),
  win: {target: 'dir', signAndEditExecutable: false}
};
