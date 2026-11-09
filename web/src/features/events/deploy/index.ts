// `features/events/deploy`: the placement of a housing or guild-house furniture (the module is registered by `features/houseM7.ts`).
export { deployOf } from './api';
export type { DeployFeature, DeployKind, DeployState } from './api';
export { createDeployController } from './controller';
export { registerDeploy } from './deployModule';
