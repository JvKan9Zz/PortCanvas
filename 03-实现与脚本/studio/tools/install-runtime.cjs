const path = require('node:path');
process.env.electron_config_cache = path.resolve(__dirname, '../../../05-测试与验证/local/electron-cache');
require('electron/install.js');
