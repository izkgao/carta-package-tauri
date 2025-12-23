const electron = require('electron');
const { app, BrowserWindow, TouchBar, Menu, shell, dialog } = electron;
const { TouchBarLabel, TouchBarButton, TouchBarSpacer } = TouchBar;
const { exec, spawn } = require('child_process');
const path = require('path');
const url = require('url');
const fs = require('fs');
const os = require('os');
const minimist = require('minimist');
const contextMenu = require('electron-context-menu');
const WindowStateManager = require('electron-window-state-manager');
const mainProcess = require('./main.js');
const uuid = require('uuid');
const getPortSync = require('find-free-port-sync');
const homedir = os.homedir();

app.allowRendererProcessReuse = true;

// To process all command line arguments correctly (Also remove the --inspect flag)
function generateExtraArgs(args) {
  const newArgs = [];

  for (const argName in args) {
    if (args.hasOwnProperty(argName) && argName !== '_' && argName !== 'inspect') {
      const value = args[argName];
      if (value === true) {
        newArgs.push(`--${argName}`);
      } else if (value !== false) {
        newArgs.push(`--${argName}=${value}`);
      }
    }
  }
  return newArgs.join(' ');
}

const items = minimist(process.argv.slice(1));
const args = minimist(process.argv.slice(1));
const inputPath = args._[0] || '';
let baseDirectory;

// Handle the different File Browser starting folder scenarios
try {
    let fileStatus = null;

    if (inputPath !== '') {
        fileStatus = fs.statSync(inputPath);
    }

    if (inputPath === '' ) {
        if (process.platform === 'darwin' && process.cwd().split('/').pop() === '') {
            baseDirectory = homedir; // When started using the Launchpad, set Filebrowser to $HOME
        } else {
            baseDirectory = process.cwd(); // When using command line without arguments, set Filebrowser to $PWD
        }
    } else if (fileStatus.isFile()) {
        fileMode = 1;
        baseDirectory = path.dirname(inputPath); // Using command line to directly open an image
    } else if (fileStatus.isDirectory()) {
        fileMode = 0;
        baseDirectory = inputPath; // When using command line with a folder argument, set Filebrowser to specified folder
    }
} catch (err) {
    if (err.code === 'ENOENT') {
        console.log('Error: Requested file or directory does not exist');
        process.exit();
    } else {
        console.log('An unexpected error occurred:', err);
        process.exit();
    }
}

// Creating simplified custom menus
const template = [
  {
    label: 'Edit',
    submenu: [
      { role: 'undo' },
      { role: 'redo' },
      { type: 'separator' },
      { role: 'cut' },
      { role: 'copy' },
      { role: 'paste' },
      { role: 'delete' },
      { type: 'separator' },
      { role: 'selectall' },
    ]
  }
]

if (process.platform === 'darwin') {
  const name = app.name;
  template.unshift({
    label: name,
    submenu: [
      {
        label: 'New CARTA Window',
        accelerator: process.platform === 'darwin' ? 'Cmd+N' : 'Ctrl+N',
        click() {
          openNewCarta()
        }
      },
      { type: 'separator' },
      { role: 'toggleFullScreen' },
      { type: 'separator' },
      { role: 'toggleDevTools' },
      { type: 'separator' },
      {
        role: 'quit'
      },
    ]
  })
}

const menu = Menu.buildFromTemplate(template);
Menu.setApplicationMenu(menu);

// macOS touch bar support
const button1 = new TouchBarButton({
    icon: path.join(__dirname, 'carta_icon_128px.png'),
    iconPosition: 'left',
    label: 'CARTA',
    backgroundColor: '#000',
});

const button2 = new TouchBarButton({
    iconPosition: 'right',
    label: 'New CARTA Window',
    click() {
        openNewCarta()
    }
});

const button3 = new TouchBarButton({
    iconPosition: 'right',
    label: 'CARTA User Manual',
    click: () => {
        shell.openExternal('https://carta.readthedocs.io/en/4.1');
    },
});

const touchBar = new TouchBar({
    items: [
        new TouchBarSpacer({ size: 'flexible' }),
        button1,
        new TouchBarSpacer({ size: 'flexible' }),
        button2,
        new TouchBarSpacer({ size: 'flexible' }),
        button3,
    ],
});

// Print the --help output from the carta_backend --help output
if (items.help) {

  var run = exec(path.join(__dirname, 'carta-backend/bin/carta_backend --help'));

  run.stdout.on('data', (data) => {
    console.log(`${data}`);
    console.log('Additional Electron version flag:');
    console.log('      --inspect      Open the DevTools in the Electron window.');
  });

  run.on('error', (err) => {
    console.error('Error:', err);
  });

  run.on('exit', () => {
    process.exit();
  });

}

// Print the --version output from the carta_backend --version output
if (items.version) {

  var run = exec(path.join(__dirname, 'carta-backend/bin/carta_backend --version'));

  run.stdout.on('data', (data) => {
    console.log(`${data}`);
  });

  run.on('error', (err) => {
    console.error('Error:', err);
  });

  run.on('exit', () => {
    process.exit();
  });

}

// Allow multiple instances of Electron
const windows = new Set();

// Generate a UUID for the CARTA_AUTH_TOKEN
const cartaAuthToken = uuid.v4();

app.on('ready', () => {
  createWindow();
});

app.on('window-all-closed', () => {
  if (process.platform === 'darwin') {
    return false;
  }
});

app.on('activate', (event, hasVisibleWindows) => {
  if (!hasVisibleWindows) { createWindow(); }
});

let newWindow;

const mainWindowState = new WindowStateManager('newWindow', {
    defaultWidth: 1920,
    defaultHeight: 1080
});

const createWindow = exports.createWindow = () => {
  let x, y;
  x =  mainWindowState.x;
  y =  mainWindowState.y;

  const currentWindow = BrowserWindow.getFocusedWindow();

  if (currentWindow) {
    const [ currentWindowX, currentWindowY ] = currentWindow.getPosition();
    x = currentWindowX + 25;
    y = currentWindowY + 25;
  }

    const newWindow = new BrowserWindow({
    width: mainWindowState.width,
    height: mainWindowState.height,
    x: x,
    y: y,
    show: false
  });

  // Using the find-free-port-sync to find a free port for each carta-backend instance
  backendPort = getPortSync();

  // Open the Electron DevTools with the --inspect flag
  if (items.inspect === true) {
    newWindow.webContents.openDevTools();
  }

  const finalExtraArgs = generateExtraArgs(items);

  const runArgs = [
    path.join(__dirname, 'carta-backend/bin/run.sh'),
    cartaAuthToken,
    baseDirectory,
    backendPort,
    finalExtraArgs
  ];

  const run = exec(runArgs.join(' '));

  // Correctly handle Electron window URL scenarios
  if (inputPath === '') {
    newWindow.loadURL(`file://${__dirname}/index.html?socketUrl=ws://localhost:${encodeURIComponent(backendPort)}&token=${encodeURIComponent(cartaAuthToken)}`);
  } else {
    if (fileMode === 1) {
      const inputFile = inputPath.startsWith('/') ? inputPath : `${process.cwd()}/${inputPath}`;
      newWindow.loadURL(`file://${__dirname}/index.html?socketUrl=ws://localhost:${encodeURIComponent(backendPort)}&token=${encodeURIComponent(cartaAuthToken)}&file=${encodeURIComponent(inputFile)}`);
    } else if (fileMode === 0) {
      newWindow.loadURL(`file://${__dirname}/index.html?socketUrl=ws://localhost:${encodeURIComponent(backendPort)}&token=${encodeURIComponent(cartaAuthToken)}`);
    }
  }

  run.stdout.on('data', (data) => {
     console.log(`${data}`);
  });

  run.on('error', (err) => {
     console.error('Error:', err);
  });

  app.releaseSingleInstanceLock();

  newWindow.once('ready-to-show', () => {
    newWindow.show();
  });

  newWindow.setTouchBar(touchBar);

  newWindow.on('close', () => {
    mainWindowState.saveState(newWindow);

   // Make sure to stop the carta_backend process when finished
   const pkill = require('child_process').spawn('/usr/bin/pkill', ['-P', run.pid]);

  });

  // Completely close Electron if no other windows are open
  app.on('window-all-closed', function () {
    app.quit();
  });

  windows.add(newWindow);
  return newWindow;
};

function openNewCarta() {
  mainProcess.createWindow();
}