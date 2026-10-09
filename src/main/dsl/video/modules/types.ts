import type { VideoValueType } from '../../../../shared/video/videoGraph';

export interface VideoParamSpec {
    values: readonly string[];
    default: string;
}

export interface VideoModuleDef {
    inputs: Record<string, VideoValueType>;
    output: VideoValueType;
    params: Record<string, VideoParamSpec>;
    /**
     * WGSL function declarations the expression calls. Emitted once per
     * module kind, so every function name must be unique to this module.
     */
    helpers?: string;
    /**
     * WGSL expression for the module's value. `args` holds one WGSL
     * expression per input, `params` the resolved option per param.
     */
    emit(args: Record<string, string>, params: Record<string, string>): string;
}
