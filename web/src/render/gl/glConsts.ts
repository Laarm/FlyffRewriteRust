// WebGL2 / extension enum values used by the texture upload paths (the numbers are fixed by the specs), so the
// planning code and its tests need no GL context. Names are the GL names.

export const GL = {
  // targets and parameters
  TEXTURE_2D: 0x0de1,
  TEXTURE_2D_ARRAY: 0x8c1a,
  TEXTURE_MIN_FILTER: 0x2801,
  TEXTURE_MAG_FILTER: 0x2800,
  TEXTURE_WRAP_S: 0x2802,
  TEXTURE_WRAP_T: 0x2803,
  TEXTURE_BASE_LEVEL: 0x813c,
  TEXTURE_MAX_LEVEL: 0x813d,
  // filters and wrapping
  NEAREST: 0x2600,
  LINEAR: 0x2601,
  NEAREST_MIPMAP_NEAREST: 0x2700,
  LINEAR_MIPMAP_NEAREST: 0x2701,
  NEAREST_MIPMAP_LINEAR: 0x2702,
  LINEAR_MIPMAP_LINEAR: 0x2703,
  REPEAT: 0x2901,
  CLAMP_TO_EDGE: 0x812f,
  // pixel store
  UNPACK_ALIGNMENT: 0x0cf5,
  UNPACK_FLIP_Y_WEBGL: 0x9240,
  UNPACK_PREMULTIPLY_ALPHA_WEBGL: 0x9241,
  UNPACK_COLORSPACE_CONVERSION_WEBGL: 0x9243,
  BROWSER_DEFAULT_WEBGL: 0x9244,
  NONE: 0,
  // formats and types
  RGBA: 0x1908,
  RGB: 0x1907,
  RED: 0x1903,
  UNSIGNED_BYTE: 0x1401,
  UNSIGNED_SHORT_5_6_5: 0x8363,
  UNSIGNED_SHORT_5_5_5_1: 0x8034,
  // sized internal formats
  RGBA8: 0x8058,
  RGB8: 0x8051,
  RGB565: 0x8d62,
  RGB5_A1: 0x8057,
  R8: 0x8229,
  // WEBGL_compressed_texture_s3tc
  COMPRESSED_RGB_S3TC_DXT1_EXT: 0x83f0,
  COMPRESSED_RGBA_S3TC_DXT1_EXT: 0x83f1,
  COMPRESSED_RGBA_S3TC_DXT3_EXT: 0x83f2,
  COMPRESSED_RGBA_S3TC_DXT5_EXT: 0x83f3,
  // EXT_texture_filter_anisotropic
  TEXTURE_MAX_ANISOTROPY_EXT: 0x84fe,
  MAX_TEXTURE_MAX_ANISOTROPY_EXT: 0x84ff,
  // errors
  NO_ERROR: 0,
} as const;

/** Bytes per 4x4 block of a compressed format. */
export function blockBytes(internalFormat: number): number {
  switch (internalFormat) {
    case GL.COMPRESSED_RGB_S3TC_DXT1_EXT:
    case GL.COMPRESSED_RGBA_S3TC_DXT1_EXT:
      return 8;
    case GL.COMPRESSED_RGBA_S3TC_DXT3_EXT:
    case GL.COMPRESSED_RGBA_S3TC_DXT5_EXT:
      return 16;
    default:
      return 0;
  }
}

/** Human-readable name of an internal format for probes and logs. */
export function internalFormatName(internalFormat: number): string {
  for (const [name, value] of Object.entries(GL)) {
    if (
      value === internalFormat &&
      (name.startsWith('COMPRESSED_') || /^(RGBA?8|RGB565|RGB5_A1|R8)$/.test(name))
    ) {
      return name;
    }
  }
  return `0x${internalFormat.toString(16)}`;
}
