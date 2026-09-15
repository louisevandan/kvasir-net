/** P4 OUTER model-loading planner. Implementation and validation live in this module. */
export { planModelLoading } from "./src/model-loading-planner.ts";
export type { AcceleratorSpecification, MachineSpecification, ModelLayerDefinition, ModelLoadingDefinition, DeviceCalibration, ModelLoadingPlannerInput, ModelLoadingPlannerResult } from "./src/model-loading-planner.ts";
export { validateNativeDeployment } from "./src/native-deployment.ts";
export type { NativeMemoryEntry, NativeMemoryPlan, NativeDeploymentInput, NativeDeploymentResult } from "./src/native-deployment.ts";
