import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { Resvg } from "@resvg/resvg-js";

const usage = "Usage: node scripts/svg-to-png/convert.mjs <input.svg> <output.png> [--size 64] [--color #20242B]";
const args = process.argv.slice(2);

if (args.length < 2) {
  console.error(usage);
  process.exit(1);
}

const [input, output, ...options] = args;
let size = 64;
let color;

for (let index = 0; index < options.length; index += 1) {
  const option = options[index];
  const value = options[index + 1];
  if (option === "--size" && /^\d+$/.test(value ?? "")) {
    size = Number(value);
    index += 1;
  } else if (option === "--color" && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(value ?? "")) {
    color = value;
    index += 1;
  } else {
    console.error(usage);
    process.exit(1);
  }
}

if (size < 1 || size > 4096) {
  console.error("Size must be between 1 and 4096 pixels.");
  process.exit(1);
}

let svg = await readFile(input, "utf8");
if (!/<svg\b/.test(svg)) {
  throw new Error(`${input} does not contain an SVG root element.`);
}
if (color) {
  svg = svg.replace(/<svg\b/, `<svg fill="${color}"`);
}

const renderer = new Resvg(svg, {
  fitTo: { mode: "width", value: size },
});
const png = renderer.render().asPng();
await mkdir(dirname(output), { recursive: true });
await writeFile(output, png);
console.log(`created ${output}`);
