// Fetches the Quaternius crop models (CC0) from Poly Pizza and normalises
// them for the simulator's standing crop.
//
//   node tools/plants/fetch.mjs
//
// Downloads each model into /tmp and calls normalise.mjs, which writes the
// metre-scaled GLBs into crates/world-render/src/plants/. The sources and
// licences are recorded in THIRD_PARTY_LICENSES.md.

import { writeFileSync } from "node:fs";
import { execFile } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const outDir = join(root, "crates", "world-render", "src", "plants");
const normalise = join(here, "normalise.mjs");

// Poly Pizza model ids, all by Quaternius under CC0 1.0.
const picks = {
  wheat: "lPspzfC8Pu",
  corn: "IhhB7kGaAQ",
  lettuce: "MEmUwHUHNR",
  grass: "UGTOzcO3P2",
  hay: "Yu8TOERkpw",
  clover: "IQ9NVyVpUw",
  turnip: "taMEmsQCye",
  flowers: "NBUxHir6FJ",
  tree: "aVOxaHRPWe",
  vines: "EVS4viM9BL",
};

function run(cmd, args) {
  return new Promise((ok, fail) =>
    execFile(cmd, args, (err, stdout, stderr) =>
      err ? fail(new Error(stderr || err.message)) : ok(stdout),
    ),
  );
}

for (const [name, id] of Object.entries(picks)) {
  const page = await fetch(`https://poly.pizza/m/${id}`, {
    headers: { "user-agent": "Mozilla/5.0 (connected-rails tools)" },
  });
  const html = await page.text();
  const urls = [
    ...new Set([...html.matchAll(/https:\/\/static\.poly\.pizza\/[\w-]+\.glb/g)].map(m => m[0])),
  ];
  if (!urls.length) {
    console.warn(`${name}: no .glb found on poly.pizza/m/${id} — the page moved; pick the new id there`);
    continue;
  }
  const bin = Buffer.from(await (await fetch(urls.at(-1))).arrayBuffer());
  const raw = `/tmp/${name}.source.glb`;
  writeFileSync(raw, bin);
  console.log(`${name}: ${(bin.length / 1024).toFixed(0)} kB fetched`);
  await run("node", [normalise, raw, join(outDir, `${name}.glb`)]);
}
