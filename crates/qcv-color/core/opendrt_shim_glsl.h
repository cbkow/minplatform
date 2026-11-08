/* SPDX-License-Identifier: GPL-3.0-only
 * minColorAE. Copyright (C) 2026 cbkow.
 * Derived from OpenDRT v1.1.0 by Jed Smith (https://github.com/jedypod/open-display-transform), GPLv3;
 * modified 2026-09-30 to 2026-10-04, see CHANGES-FROM-OPENDRT.md. Not affiliated with or endorsed by OpenDRT.
 */
/* DRT core — GLSL (4.40 / Vulkan) shim.
 *
 * STATUS: NOT YET COMPILE-CHECKED. No glslangValidator on the machine this was
 * written on. a host's QRhi window path (GLSL 440 through QShaderBaker) is the
 * first consumer and will validate it.
 *
 * Host source:
 *   #version 440
 *   #include-equivalent: paste this file, then opendrt_params.h, then opendrt_kernel.h
 *   layout(std140, binding = N) uniform DrtBlock { DrtParams drtParams; };
 *   ... drt_transform(drtParams, rgb)
 *
 * Two things GLSL does differently from every other dialect, both handled here:
 *   - mod(x, y) floors; C fmod truncates. drt_hue_offset feeds negative values in,
 *     so the sign convention matters. drt_cfmod reproduces fmod.
 *   - there is no exp10; pow(10, x) stands in.
 * GLSL also has no `static`, hence the empty __DEVICE__.
 */

#define __DEVICE__
#define __CONSTANT__ const

#define float2 vec2
#define float3 vec3
#define make_float2(a, b)    vec2((a), (b))
#define make_float3(a, b, c) vec3((a), (b), (c))

float drt_cfmod(float a, float b) { return a - b * trunc(a / b); }

#define _powf(a, b)   pow((a), (b))
#define _expf(a)      exp((a))
#define _exp2f(a)     exp2((a))
#define _exp10f(a)    pow(10.0f, (a))
#define _logf(a)      log((a))
#define _log2f(a)     log2((a))
#define _sqrtf(a)     sqrt((a))
#define _fabs(a)      abs((a))
#define _fmod(a, b)   drt_cfmod((a), (b))
#define _floorf(a)    floor((a))
#define _atan2f(a, b) atan((a), (b))
#define _fminf(a, b)  min((a), (b))
#define _fmaxf(a, b)  max((a), (b))

#define DRT_PARAMS_ARG DrtParams
#define DRT_AGX_ARG DrtAgxParams
