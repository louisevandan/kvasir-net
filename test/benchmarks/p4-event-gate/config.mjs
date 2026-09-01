import fs from "node:fs/promises";
import path from "node:path";

export async function loadConfig(configPath) {
  const config = JSON.parse(await fs.readFile(configPath, "utf8"));
  await expandArrayFile(config, "prompts", "prompts_file", configPath, isString, "config");
  if (config.acceptance) {
    await expandArrayFile(
      config.acceptance,
      "responses",
      "responses_file",
      configPath,
      isObject,
      "acceptance",
    );
  }
  return config;
}

async function expandArrayFile(target, valueKey, fileKey, configPath, predicate, owner) {
  if (target[fileKey] === undefined) return;
  if (target[valueKey] !== undefined) {
    throw new Error(`${owner} must use either ${valueKey} or ${fileKey}, not both`);
  }
  const filePath = path.resolve(path.dirname(configPath), target[fileKey]);
  const values = JSON.parse(await fs.readFile(filePath, "utf8"));
  if (!Array.isArray(values) || values.some((value) => !predicate(value))) {
    throw new Error(`${owner}.${fileKey} must contain a JSON ${valueKey} array`);
  }
  target[valueKey] = values;
  delete target[fileKey];
}

function isString(value) {
  return typeof value === "string";
}

function isObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
