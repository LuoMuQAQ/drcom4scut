import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const targetFile = path.join(__dirname, 'node_modules/@heroui/styles/dist/utilities/index.css');

if (fs.existsSync(targetFile)) {
  const content = fs.readFileSync(targetFile, 'utf8');
  if (!content.includes('wrap-break-word')) {
    fs.appendFileSync(targetFile, '\n@utility wrap-break-word {\n  overflow-wrap: break-word;\n}\n');
    console.log('[HeroUI Patch] Successfully patched wrap-break-word into @heroui/styles');
  }
}
