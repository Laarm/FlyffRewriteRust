// Constants of the shop windows that other modules read (kept out of the lazily loaded shells; WP-UI-26, WP-UI-37).

/** `m_textureMng.AddTexture( .., "WndVenderArrowEx.tga" )`: the focus marker of the price window, 128 x 24. */
export const FOCUS_MARKER = 'theme/WndVenderArrowEx.tga';
export const FOCUS_MARKER_SIZE = { w: 128, h: 24 } as const;
/** The price of the VENMAX key (`2100000000`) and of a result that wrapped. */
export const PRICE_MAX = 2_100_000_000;
/** `strlen( pWndText->m_string ) < 1020`: the longest line the chat window sends. */
export const CHAT_LINE_MAX = 1019;
