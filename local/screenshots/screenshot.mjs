// Screenshots of the provisioner UI against the lab (local/lab/), logged in through the local
// Keycloak (local/keycloak/run-keycloak.sh) as admin/admin, or as LOGIN_USER (its password is
// the same, `reader` is the user not in auth_users).
//
//   local/screenshots/run.sh [--out DIR] STEP...
//
// Steps, run in order after the login:
//   goto:/path              open a page of the provisioner
//   click:Text              click the button (or else the element) with this text
//   clicklast:Text          click the last element with this text (a card title after links)
//   clickrow:Text=Label     click the button with this label in the table row containing Text
//   fill:Placeholder=Value  type into the input with this placeholder
//   waitfor:Text|Other      wait until one of the texts shows up (WAIT_TIMEOUT_MS, 5 min)
//   wait:ms                 wait a fixed time
//   top                     scroll everything back to the top
//   shot:name               store a full-page PNG
// The login page and the first page after it are always captured. The viewport is
// VIEWPORT_WIDTH x VIEWPORT_HEIGHT (default 1440 x 1800); 390 is a phone.
import { chromium } from 'playwright';
import fs from 'node:fs';

const args = process.argv.slice(2);
const outIndex = args.indexOf('--out');
const out = outIndex >= 0 ? args.splice(outIndex, 2)[1] : 'screenshots';
const origin = process.env.PROVISIONER_URL ?? 'http://localhost:8082';
fs.mkdirSync(out, { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: Number(process.env.VIEWPORT_WIDTH ?? 1440), height: Number(process.env.VIEWPORT_HEIGHT ?? 1800) } });
page.on('console', (m) => m.type() === 'error' && console.log('  console error:', m.text()));
page.on('pageerror', (e) => console.log('  page error:', e.message));

let shot = 0;
const capture = async (name) => {
  const file = `${out}/${String(++shot).padStart(2, '0')}-${name.replace(/\W+/g, '_').replace(/^_|_$/g, '') || 'index'}.png`;
  await page.screenshot({ path: file, fullPage: true });
  console.log(`-> ${file}`);
};

await page.goto(origin);
await page.waitForSelector('#username', { timeout: 30000 });
await capture('login');
const user = process.env.LOGIN_USER ?? 'admin';
await page.fill('#username', user);
await page.fill('#password', user);
await page.click('#kc-login');
await page.waitForURL((url) => url.origin === origin, { timeout: 30000 });
await page.waitForTimeout(3000);
await capture('logged-in');

for (const step of args) {
  const [kind, ...rest] = step.split(':');
  const value = rest.join(':');
  console.log(step);
  switch (kind) {
    case 'goto':
      await page.goto(origin + value);
      await page.waitForLoadState('load');
      break;
    case 'click': {
      const button = page.getByRole('button', { name: value, exact: true });
      await ((await button.count()) ? button.first() : page.getByText(value, { exact: true }).first()).click();
      break;
    }
    case 'clicklast':
      await page.getByText(value, { exact: true }).last().click();
      break;
    case 'clickrow': {
      const [row, ...label] = value.split('=');
      await page.locator('tr', { hasText: row })
        .getByRole('button', { name: label.join('='), exact: true }).first().click();
      break;
    }
    case 'fill': {
      const [placeholder, ...text] = value.split('=');
      const input = page.getByPlaceholder(placeholder);
      await input.fill(text.join('='));
      await input.press('Tab');
      break;
    }
    case 'waitfor':
      await value
        .split('|')
        .map((text) => page.getByText(text))
        .reduce((a, b) => a.or(b))
        .first()
        .waitFor({ timeout: Number(process.env.WAIT_TIMEOUT_MS ?? 300000) });
      break;
    case 'wait':
      await page.waitForTimeout(Number(value));
      break;
    case 'top':
      // the page scrolls in its own container, which a full-page shot doesn't unroll
      await page.evaluate(() => {
        for (const element of document.querySelectorAll('*')) element.scrollTop = 0;
      });
      break;
    case 'shot':
      await capture(value);
      break;
    default:
      throw new Error(`unknown step ${step}`);
  }
}
await browser.close();
