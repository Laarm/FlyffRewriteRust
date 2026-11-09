// Constants of the readable-item windows that other modules read (kept out of the lazily loaded shells; WP-UI-37).

/** The picture of each kind (a theme file, full precision, painted where `WIDC_STATIC1` is). */
export const READABLE_PICTURE: Readonly<Record<'book' | 'scroll' | 'letter', string>> = {
  book: 'theme/subBook.tga',
  scroll: 'theme/subScroll.tga',
  letter: 'theme/subLetter.tga',
};
