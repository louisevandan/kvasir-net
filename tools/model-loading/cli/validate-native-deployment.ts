import fs from "node:fs";
import path from "node:path";
import { validateNativeDeployment, type NativeDeploymentInput } from "../index.ts";

const [inputPath, outputPath] = process.argv.slice(2);
if (!inputPath) throw new Error("usage: node validate-native-deployment.ts INPUT.json [RESULT.json]");
const input = JSON.parse(fs.readFileSync(path.resolve(inputPath), "utf8")) as NativeDeploymentInput;
const result = validateNativeDeployment(input);
const text = JSON.stringify(result, null, 2) + "\n";
if (outputPath) {
  fs.writeFileSync(path.resolve(outputPath), text, { flag: "wx" });
} else {
  process.stdout.write(text);
}
