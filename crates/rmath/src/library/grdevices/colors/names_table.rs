//! colors — extracted verbatim from colors.rs.

use super::*;

// ---------------------------------------------------------------------------
// Color name database
// ---------------------------------------------------------------------------

pub(crate) struct ColorDataBaseEntry {
    pub(crate) name: &'static [u8],
    pub(crate) code: rcolor,
}

pub(crate) static COLOR_DATA_BASE: &[ColorDataBaseEntry] = &COLOR_DATA;

// The actual database is defined as a const array below
pub(crate) const COLOR_DATA: [ColorDataBaseEntry; 657] = [
    ColorDataBaseEntry {
        name: b"white\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"aliceblue\0",
        code: 0xfffff8f0,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite\0",
        code: 0xffd7ebfa,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite1\0",
        code: 0xffdbefff,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite2\0",
        code: 0xffccdfee,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite3\0",
        code: 0xffb0c0cd,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite4\0",
        code: 0xff78838b,
    },
    ColorDataBaseEntry {
        name: b"aquamarine\0",
        code: 0xffd4ff7f,
    },
    ColorDataBaseEntry {
        name: b"aquamarine1\0",
        code: 0xffd4ff7f,
    },
    ColorDataBaseEntry {
        name: b"aquamarine2\0",
        code: 0xffc6ee76,
    },
    ColorDataBaseEntry {
        name: b"aquamarine3\0",
        code: 0xffaacd66,
    },
    ColorDataBaseEntry {
        name: b"aquamarine4\0",
        code: 0xff748b45,
    },
    ColorDataBaseEntry {
        name: b"azure\0",
        code: 0xfffffff0,
    },
    ColorDataBaseEntry {
        name: b"azure1\0",
        code: 0xfffffff0,
    },
    ColorDataBaseEntry {
        name: b"azure2\0",
        code: 0xffeeeee0,
    },
    ColorDataBaseEntry {
        name: b"azure3\0",
        code: 0xffcdcdc1,
    },
    ColorDataBaseEntry {
        name: b"azure4\0",
        code: 0xff8b8b83,
    },
    ColorDataBaseEntry {
        name: b"beige\0",
        code: 0xffdcf5f5,
    },
    ColorDataBaseEntry {
        name: b"bisque\0",
        code: 0xffc4e4ff,
    },
    ColorDataBaseEntry {
        name: b"bisque1\0",
        code: 0xffc4e4ff,
    },
    ColorDataBaseEntry {
        name: b"bisque2\0",
        code: 0xffb7d5ee,
    },
    ColorDataBaseEntry {
        name: b"bisque3\0",
        code: 0xff9eb7cd,
    },
    ColorDataBaseEntry {
        name: b"bisque4\0",
        code: 0xff6b7d8b,
    },
    ColorDataBaseEntry {
        name: b"black\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"blanchedalmond\0",
        code: 0xffcdebff,
    },
    ColorDataBaseEntry {
        name: b"blue\0",
        code: 0xffff0000,
    },
    ColorDataBaseEntry {
        name: b"blue1\0",
        code: 0xffff0000,
    },
    ColorDataBaseEntry {
        name: b"blue2\0",
        code: 0xffee0000,
    },
    ColorDataBaseEntry {
        name: b"blue3\0",
        code: 0xffcd0000,
    },
    ColorDataBaseEntry {
        name: b"blue4\0",
        code: 0xff8b0000,
    },
    ColorDataBaseEntry {
        name: b"blueviolet\0",
        code: 0xffe22b8a,
    },
    ColorDataBaseEntry {
        name: b"brown\0",
        code: 0xff2a2aa5,
    },
    ColorDataBaseEntry {
        name: b"brown1\0",
        code: 0xff4040ff,
    },
    ColorDataBaseEntry {
        name: b"brown2\0",
        code: 0xff3b3bee,
    },
    ColorDataBaseEntry {
        name: b"brown3\0",
        code: 0xff3333cd,
    },
    ColorDataBaseEntry {
        name: b"brown4\0",
        code: 0xff23238b,
    },
    ColorDataBaseEntry {
        name: b"burlywood\0",
        code: 0xff87b8de,
    },
    ColorDataBaseEntry {
        name: b"burlywood1\0",
        code: 0xff9bd3ff,
    },
    ColorDataBaseEntry {
        name: b"burlywood2\0",
        code: 0xff91c5ee,
    },
    ColorDataBaseEntry {
        name: b"burlywood3\0",
        code: 0xff7daacd,
    },
    ColorDataBaseEntry {
        name: b"burlywood4\0",
        code: 0xff55738b,
    },
    ColorDataBaseEntry {
        name: b"cadetblue\0",
        code: 0xffa09e5f,
    },
    ColorDataBaseEntry {
        name: b"cadetblue1\0",
        code: 0xfffff598,
    },
    ColorDataBaseEntry {
        name: b"cadetblue2\0",
        code: 0xffeee58e,
    },
    ColorDataBaseEntry {
        name: b"cadetblue3\0",
        code: 0xffcdc57a,
    },
    ColorDataBaseEntry {
        name: b"cadetblue4\0",
        code: 0xff8b8653,
    },
    ColorDataBaseEntry {
        name: b"chartreuse\0",
        code: 0xff00ff7f,
    },
    ColorDataBaseEntry {
        name: b"chartreuse1\0",
        code: 0xff00ff7f,
    },
    ColorDataBaseEntry {
        name: b"chartreuse2\0",
        code: 0xff00ee76,
    },
    ColorDataBaseEntry {
        name: b"chartreuse3\0",
        code: 0xff00cd66,
    },
    ColorDataBaseEntry {
        name: b"chartreuse4\0",
        code: 0xff008b45,
    },
    ColorDataBaseEntry {
        name: b"chocolate\0",
        code: 0xff1e69d2,
    },
    ColorDataBaseEntry {
        name: b"chocolate1\0",
        code: 0xff247fff,
    },
    ColorDataBaseEntry {
        name: b"chocolate2\0",
        code: 0xff2176ee,
    },
    ColorDataBaseEntry {
        name: b"chocolate3\0",
        code: 0xff1d66cd,
    },
    ColorDataBaseEntry {
        name: b"chocolate4\0",
        code: 0xff13458b,
    },
    ColorDataBaseEntry {
        name: b"coral\0",
        code: 0xff507fff,
    },
    ColorDataBaseEntry {
        name: b"coral1\0",
        code: 0xff5672ff,
    },
    ColorDataBaseEntry {
        name: b"coral2\0",
        code: 0xff506aee,
    },
    ColorDataBaseEntry {
        name: b"coral3\0",
        code: 0xff455bcd,
    },
    ColorDataBaseEntry {
        name: b"coral4\0",
        code: 0xff2f3e8b,
    },
    ColorDataBaseEntry {
        name: b"cornflowerblue\0",
        code: 0xffed9564,
    },
    ColorDataBaseEntry {
        name: b"cornsilk\0",
        code: 0xffdcf8ff,
    },
    ColorDataBaseEntry {
        name: b"cornsilk1\0",
        code: 0xffdcf8ff,
    },
    ColorDataBaseEntry {
        name: b"cornsilk2\0",
        code: 0xffcde8ee,
    },
    ColorDataBaseEntry {
        name: b"cornsilk3\0",
        code: 0xffb1c8cd,
    },
    ColorDataBaseEntry {
        name: b"cornsilk4\0",
        code: 0xff78888b,
    },
    ColorDataBaseEntry {
        name: b"cyan\0",
        code: 0xffffff00,
    },
    ColorDataBaseEntry {
        name: b"cyan1\0",
        code: 0xffffff00,
    },
    ColorDataBaseEntry {
        name: b"cyan2\0",
        code: 0xffeeee00,
    },
    ColorDataBaseEntry {
        name: b"cyan3\0",
        code: 0xffcdcd00,
    },
    ColorDataBaseEntry {
        name: b"cyan4\0",
        code: 0xff8b8b00,
    },
    ColorDataBaseEntry {
        name: b"darkblue\0",
        code: 0xff8b0000,
    },
    ColorDataBaseEntry {
        name: b"darkcyan\0",
        code: 0xff8b8b00,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod\0",
        code: 0xff0b86b8,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod1\0",
        code: 0xff0fb9ff,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod2\0",
        code: 0xff0eadee,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod3\0",
        code: 0xff0c95cd,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod4\0",
        code: 0xff08658b,
    },
    ColorDataBaseEntry {
        name: b"darkgray\0",
        code: 0xffa9a9a9,
    },
    ColorDataBaseEntry {
        name: b"darkgreen\0",
        code: 0xff006400,
    },
    ColorDataBaseEntry {
        name: b"darkgrey\0",
        code: 0xffa9a9a9,
    },
    ColorDataBaseEntry {
        name: b"darkkhaki\0",
        code: 0xff6bb7bd,
    },
    ColorDataBaseEntry {
        name: b"darkmagenta\0",
        code: 0xff8b008b,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen\0",
        code: 0xff2f6b55,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen1\0",
        code: 0xff70ffca,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen2\0",
        code: 0xff68eebc,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen3\0",
        code: 0xff5acda2,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen4\0",
        code: 0xff3d8b6e,
    },
    ColorDataBaseEntry {
        name: b"darkorange\0",
        code: 0xff008cff,
    },
    ColorDataBaseEntry {
        name: b"darkorange1\0",
        code: 0xff007fff,
    },
    ColorDataBaseEntry {
        name: b"darkorange2\0",
        code: 0xff0076ee,
    },
    ColorDataBaseEntry {
        name: b"darkorange3\0",
        code: 0xff0066cd,
    },
    ColorDataBaseEntry {
        name: b"darkorange4\0",
        code: 0xff00458b,
    },
    ColorDataBaseEntry {
        name: b"darkorchid\0",
        code: 0xffcc3299,
    },
    ColorDataBaseEntry {
        name: b"darkorchid1\0",
        code: 0xffff3ebf,
    },
    ColorDataBaseEntry {
        name: b"darkorchid2\0",
        code: 0xffee3ab2,
    },
    ColorDataBaseEntry {
        name: b"darkorchid3\0",
        code: 0xffcd329a,
    },
    ColorDataBaseEntry {
        name: b"darkorchid4\0",
        code: 0xff8b2268,
    },
    ColorDataBaseEntry {
        name: b"darkred\0",
        code: 0xff00008b,
    },
    ColorDataBaseEntry {
        name: b"darksalmon\0",
        code: 0xff7a96e9,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen\0",
        code: 0xff8fbc8f,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen1\0",
        code: 0xffc1ffc1,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen2\0",
        code: 0xffb4eeb4,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen3\0",
        code: 0xff9bcd9b,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen4\0",
        code: 0xff698b69,
    },
    ColorDataBaseEntry {
        name: b"darkslateblue\0",
        code: 0xff8b3d48,
    },
    ColorDataBaseEntry {
        name: b"darkslategray\0",
        code: 0xff4f4f2f,
    },
    ColorDataBaseEntry {
        name: b"darkslategray1\0",
        code: 0xffffff97,
    },
    ColorDataBaseEntry {
        name: b"darkslategray2\0",
        code: 0xffeeee8d,
    },
    ColorDataBaseEntry {
        name: b"darkslategray3\0",
        code: 0xffcdcd79,
    },
    ColorDataBaseEntry {
        name: b"darkslategray4\0",
        code: 0xff8b8b52,
    },
    ColorDataBaseEntry {
        name: b"darkslategrey\0",
        code: 0xff4f4f2f,
    },
    ColorDataBaseEntry {
        name: b"darkturquoise\0",
        code: 0xffd1ce00,
    },
    ColorDataBaseEntry {
        name: b"darkviolet\0",
        code: 0xffd30094,
    },
    ColorDataBaseEntry {
        name: b"deeppink\0",
        code: 0xff9314ff,
    },
    ColorDataBaseEntry {
        name: b"deeppink1\0",
        code: 0xff9314ff,
    },
    ColorDataBaseEntry {
        name: b"deeppink2\0",
        code: 0xff8912ee,
    },
    ColorDataBaseEntry {
        name: b"deeppink3\0",
        code: 0xff7610cd,
    },
    ColorDataBaseEntry {
        name: b"deeppink4\0",
        code: 0xff500a8b,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue\0",
        code: 0xffffbf00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue1\0",
        code: 0xffffbf00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue2\0",
        code: 0xffeeb200,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue3\0",
        code: 0xffcd9a00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue4\0",
        code: 0xff8b6800,
    },
    ColorDataBaseEntry {
        name: b"dimgray\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"dimgrey\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue\0",
        code: 0xffff901e,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue1\0",
        code: 0xffff901e,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue2\0",
        code: 0xffee861c,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue3\0",
        code: 0xffcd7418,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue4\0",
        code: 0xff8b4e10,
    },
    ColorDataBaseEntry {
        name: b"firebrick\0",
        code: 0xff2222b2,
    },
    ColorDataBaseEntry {
        name: b"firebrick1\0",
        code: 0xff3030ff,
    },
    ColorDataBaseEntry {
        name: b"firebrick2\0",
        code: 0xff2c2cee,
    },
    ColorDataBaseEntry {
        name: b"firebrick3\0",
        code: 0xff2626cd,
    },
    ColorDataBaseEntry {
        name: b"firebrick4\0",
        code: 0xff1a1a8b,
    },
    ColorDataBaseEntry {
        name: b"floralwhite\0",
        code: 0xfff0faff,
    },
    ColorDataBaseEntry {
        name: b"forestgreen\0",
        code: 0xff228b22,
    },
    ColorDataBaseEntry {
        name: b"gainsboro\0",
        code: 0xffdcdcdc,
    },
    ColorDataBaseEntry {
        name: b"ghostwhite\0",
        code: 0xfffff8f8,
    },
    ColorDataBaseEntry {
        name: b"gold\0",
        code: 0xff00d7ff,
    },
    ColorDataBaseEntry {
        name: b"gold1\0",
        code: 0xff00d7ff,
    },
    ColorDataBaseEntry {
        name: b"gold2\0",
        code: 0xff00c9ee,
    },
    ColorDataBaseEntry {
        name: b"gold3\0",
        code: 0xff00adcd,
    },
    ColorDataBaseEntry {
        name: b"gold4\0",
        code: 0xff00758b,
    },
    ColorDataBaseEntry {
        name: b"goldenrod\0",
        code: 0xff20a5da,
    },
    ColorDataBaseEntry {
        name: b"goldenrod1\0",
        code: 0xff25c1ff,
    },
    ColorDataBaseEntry {
        name: b"goldenrod2\0",
        code: 0xff22b4ee,
    },
    ColorDataBaseEntry {
        name: b"goldenrod3\0",
        code: 0xff1d9bcd,
    },
    ColorDataBaseEntry {
        name: b"goldenrod4\0",
        code: 0xff14698b,
    },
    ColorDataBaseEntry {
        name: b"gray\0",
        code: 0xffbebebe,
    },
    ColorDataBaseEntry {
        name: b"gray0\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"gray1\0",
        code: 0xff030303,
    },
    ColorDataBaseEntry {
        name: b"gray2\0",
        code: 0xff050505,
    },
    ColorDataBaseEntry {
        name: b"gray3\0",
        code: 0xff080808,
    },
    ColorDataBaseEntry {
        name: b"gray4\0",
        code: 0xff0a0a0a,
    },
    ColorDataBaseEntry {
        name: b"gray5\0",
        code: 0xff0d0d0d,
    },
    ColorDataBaseEntry {
        name: b"gray6\0",
        code: 0xff0f0f0f,
    },
    ColorDataBaseEntry {
        name: b"gray7\0",
        code: 0xff121212,
    },
    ColorDataBaseEntry {
        name: b"gray8\0",
        code: 0xff141414,
    },
    ColorDataBaseEntry {
        name: b"gray9\0",
        code: 0xff171717,
    },
    ColorDataBaseEntry {
        name: b"gray10\0",
        code: 0xff1a1a1a,
    },
    ColorDataBaseEntry {
        name: b"gray11\0",
        code: 0xff1c1c1c,
    },
    ColorDataBaseEntry {
        name: b"gray12\0",
        code: 0xff1f1f1f,
    },
    ColorDataBaseEntry {
        name: b"gray13\0",
        code: 0xff212121,
    },
    ColorDataBaseEntry {
        name: b"gray14\0",
        code: 0xff242424,
    },
    ColorDataBaseEntry {
        name: b"gray15\0",
        code: 0xff262626,
    },
    ColorDataBaseEntry {
        name: b"gray16\0",
        code: 0xff292929,
    },
    ColorDataBaseEntry {
        name: b"gray17\0",
        code: 0xff2b2b2b,
    },
    ColorDataBaseEntry {
        name: b"gray18\0",
        code: 0xff2e2e2e,
    },
    ColorDataBaseEntry {
        name: b"gray19\0",
        code: 0xff303030,
    },
    ColorDataBaseEntry {
        name: b"gray20\0",
        code: 0xff333333,
    },
    ColorDataBaseEntry {
        name: b"gray21\0",
        code: 0xff363636,
    },
    ColorDataBaseEntry {
        name: b"gray22\0",
        code: 0xff383838,
    },
    ColorDataBaseEntry {
        name: b"gray23\0",
        code: 0xff3b3b3b,
    },
    ColorDataBaseEntry {
        name: b"gray24\0",
        code: 0xff3d3d3d,
    },
    ColorDataBaseEntry {
        name: b"gray25\0",
        code: 0xff404040,
    },
    ColorDataBaseEntry {
        name: b"gray26\0",
        code: 0xff424242,
    },
    ColorDataBaseEntry {
        name: b"gray27\0",
        code: 0xff454545,
    },
    ColorDataBaseEntry {
        name: b"gray28\0",
        code: 0xff474747,
    },
    ColorDataBaseEntry {
        name: b"gray29\0",
        code: 0xff4a4a4a,
    },
    ColorDataBaseEntry {
        name: b"gray30\0",
        code: 0xff4d4d4d,
    },
    ColorDataBaseEntry {
        name: b"gray31\0",
        code: 0xff4f4f4f,
    },
    ColorDataBaseEntry {
        name: b"gray32\0",
        code: 0xff525252,
    },
    ColorDataBaseEntry {
        name: b"gray33\0",
        code: 0xff545454,
    },
    ColorDataBaseEntry {
        name: b"gray34\0",
        code: 0xff575757,
    },
    ColorDataBaseEntry {
        name: b"gray35\0",
        code: 0xff595959,
    },
    ColorDataBaseEntry {
        name: b"gray36\0",
        code: 0xff5c5c5c,
    },
    ColorDataBaseEntry {
        name: b"gray37\0",
        code: 0xff5e5e5e,
    },
    ColorDataBaseEntry {
        name: b"gray38\0",
        code: 0xff616161,
    },
    ColorDataBaseEntry {
        name: b"gray39\0",
        code: 0xff636363,
    },
    ColorDataBaseEntry {
        name: b"gray40\0",
        code: 0xff666666,
    },
    ColorDataBaseEntry {
        name: b"gray41\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"gray42\0",
        code: 0xff6b6b6b,
    },
    ColorDataBaseEntry {
        name: b"gray43\0",
        code: 0xff6e6e6e,
    },
    ColorDataBaseEntry {
        name: b"gray44\0",
        code: 0xff707070,
    },
    ColorDataBaseEntry {
        name: b"gray45\0",
        code: 0xff737373,
    },
    ColorDataBaseEntry {
        name: b"gray46\0",
        code: 0xff757575,
    },
    ColorDataBaseEntry {
        name: b"gray47\0",
        code: 0xff787878,
    },
    ColorDataBaseEntry {
        name: b"gray48\0",
        code: 0xff7a7a7a,
    },
    ColorDataBaseEntry {
        name: b"gray49\0",
        code: 0xff7d7d7d,
    },
    ColorDataBaseEntry {
        name: b"gray50\0",
        code: 0xff7f7f7f,
    },
    ColorDataBaseEntry {
        name: b"gray51\0",
        code: 0xff828282,
    },
    ColorDataBaseEntry {
        name: b"gray52\0",
        code: 0xff858585,
    },
    ColorDataBaseEntry {
        name: b"gray53\0",
        code: 0xff878787,
    },
    ColorDataBaseEntry {
        name: b"gray54\0",
        code: 0xff8a8a8a,
    },
    ColorDataBaseEntry {
        name: b"gray55\0",
        code: 0xff8c8c8c,
    },
    ColorDataBaseEntry {
        name: b"gray56\0",
        code: 0xff8f8f8f,
    },
    ColorDataBaseEntry {
        name: b"gray57\0",
        code: 0xff919191,
    },
    ColorDataBaseEntry {
        name: b"gray58\0",
        code: 0xff949494,
    },
    ColorDataBaseEntry {
        name: b"gray59\0",
        code: 0xff969696,
    },
    ColorDataBaseEntry {
        name: b"gray60\0",
        code: 0xff999999,
    },
    ColorDataBaseEntry {
        name: b"gray61\0",
        code: 0xff9c9c9c,
    },
    ColorDataBaseEntry {
        name: b"gray62\0",
        code: 0xff9e9e9e,
    },
    ColorDataBaseEntry {
        name: b"gray63\0",
        code: 0xffa1a1a1,
    },
    ColorDataBaseEntry {
        name: b"gray64\0",
        code: 0xffa3a3a3,
    },
    ColorDataBaseEntry {
        name: b"gray65\0",
        code: 0xffa6a6a6,
    },
    ColorDataBaseEntry {
        name: b"gray66\0",
        code: 0xffa8a8a8,
    },
    ColorDataBaseEntry {
        name: b"gray67\0",
        code: 0xffababab,
    },
    ColorDataBaseEntry {
        name: b"gray68\0",
        code: 0xffadadad,
    },
    ColorDataBaseEntry {
        name: b"gray69\0",
        code: 0xffb0b0b0,
    },
    ColorDataBaseEntry {
        name: b"gray70\0",
        code: 0xffb3b3b3,
    },
    ColorDataBaseEntry {
        name: b"gray71\0",
        code: 0xffb5b5b5,
    },
    ColorDataBaseEntry {
        name: b"gray72\0",
        code: 0xffb8b8b8,
    },
    ColorDataBaseEntry {
        name: b"gray73\0",
        code: 0xffbababa,
    },
    ColorDataBaseEntry {
        name: b"gray74\0",
        code: 0xffbdbdbd,
    },
    ColorDataBaseEntry {
        name: b"gray75\0",
        code: 0xffbfbfbf,
    },
    ColorDataBaseEntry {
        name: b"gray76\0",
        code: 0xffc2c2c2,
    },
    ColorDataBaseEntry {
        name: b"gray77\0",
        code: 0xffc4c4c4,
    },
    ColorDataBaseEntry {
        name: b"gray78\0",
        code: 0xffc7c7c7,
    },
    ColorDataBaseEntry {
        name: b"gray79\0",
        code: 0xffc9c9c9,
    },
    ColorDataBaseEntry {
        name: b"gray80\0",
        code: 0xffcccccc,
    },
    ColorDataBaseEntry {
        name: b"gray81\0",
        code: 0xffcfcfcf,
    },
    ColorDataBaseEntry {
        name: b"gray82\0",
        code: 0xffd1d1d1,
    },
    ColorDataBaseEntry {
        name: b"gray83\0",
        code: 0xffd4d4d4,
    },
    ColorDataBaseEntry {
        name: b"gray84\0",
        code: 0xffd6d6d6,
    },
    ColorDataBaseEntry {
        name: b"gray85\0",
        code: 0xffd9d9d9,
    },
    ColorDataBaseEntry {
        name: b"gray86\0",
        code: 0xffdbdbdb,
    },
    ColorDataBaseEntry {
        name: b"gray87\0",
        code: 0xffdedede,
    },
    ColorDataBaseEntry {
        name: b"gray88\0",
        code: 0xffe0e0e0,
    },
    ColorDataBaseEntry {
        name: b"gray89\0",
        code: 0xffe3e3e3,
    },
    ColorDataBaseEntry {
        name: b"gray90\0",
        code: 0xffe5e5e5,
    },
    ColorDataBaseEntry {
        name: b"gray91\0",
        code: 0xffe8e8e8,
    },
    ColorDataBaseEntry {
        name: b"gray92\0",
        code: 0xffebebeb,
    },
    ColorDataBaseEntry {
        name: b"gray93\0",
        code: 0xffededed,
    },
    ColorDataBaseEntry {
        name: b"gray94\0",
        code: 0xfff0f0f0,
    },
    ColorDataBaseEntry {
        name: b"gray95\0",
        code: 0xfff2f2f2,
    },
    ColorDataBaseEntry {
        name: b"gray96\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"gray97\0",
        code: 0xfff7f7f7,
    },
    ColorDataBaseEntry {
        name: b"gray98\0",
        code: 0xfffafafa,
    },
    ColorDataBaseEntry {
        name: b"gray99\0",
        code: 0xfffcfcfc,
    },
    ColorDataBaseEntry {
        name: b"gray100\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"green\0",
        code: 0xff00ff00,
    },
    ColorDataBaseEntry {
        name: b"green1\0",
        code: 0xff00ff00,
    },
    ColorDataBaseEntry {
        name: b"green2\0",
        code: 0xff00ee00,
    },
    ColorDataBaseEntry {
        name: b"green3\0",
        code: 0xff00cd00,
    },
    ColorDataBaseEntry {
        name: b"green4\0",
        code: 0xff008b00,
    },
    ColorDataBaseEntry {
        name: b"greenyellow\0",
        code: 0xff2fffad,
    },
    ColorDataBaseEntry {
        name: b"grey\0",
        code: 0xffbebebe,
    },
    ColorDataBaseEntry {
        name: b"grey0\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"grey1\0",
        code: 0xff030303,
    },
    ColorDataBaseEntry {
        name: b"grey2\0",
        code: 0xff050505,
    },
    ColorDataBaseEntry {
        name: b"grey3\0",
        code: 0xff080808,
    },
    ColorDataBaseEntry {
        name: b"grey4\0",
        code: 0xff0a0a0a,
    },
    ColorDataBaseEntry {
        name: b"grey5\0",
        code: 0xff0d0d0d,
    },
    ColorDataBaseEntry {
        name: b"grey6\0",
        code: 0xff0f0f0f,
    },
    ColorDataBaseEntry {
        name: b"grey7\0",
        code: 0xff121212,
    },
    ColorDataBaseEntry {
        name: b"grey8\0",
        code: 0xff141414,
    },
    ColorDataBaseEntry {
        name: b"grey9\0",
        code: 0xff171717,
    },
    ColorDataBaseEntry {
        name: b"grey10\0",
        code: 0xff1a1a1a,
    },
    ColorDataBaseEntry {
        name: b"grey11\0",
        code: 0xff1c1c1c,
    },
    ColorDataBaseEntry {
        name: b"grey12\0",
        code: 0xff1f1f1f,
    },
    ColorDataBaseEntry {
        name: b"grey13\0",
        code: 0xff212121,
    },
    ColorDataBaseEntry {
        name: b"grey14\0",
        code: 0xff242424,
    },
    ColorDataBaseEntry {
        name: b"grey15\0",
        code: 0xff262626,
    },
    ColorDataBaseEntry {
        name: b"grey16\0",
        code: 0xff292929,
    },
    ColorDataBaseEntry {
        name: b"grey17\0",
        code: 0xff2b2b2b,
    },
    ColorDataBaseEntry {
        name: b"grey18\0",
        code: 0xff2e2e2e,
    },
    ColorDataBaseEntry {
        name: b"grey19\0",
        code: 0xff303030,
    },
    ColorDataBaseEntry {
        name: b"grey20\0",
        code: 0xff333333,
    },
    ColorDataBaseEntry {
        name: b"grey21\0",
        code: 0xff363636,
    },
    ColorDataBaseEntry {
        name: b"grey22\0",
        code: 0xff383838,
    },
    ColorDataBaseEntry {
        name: b"grey23\0",
        code: 0xff3b3b3b,
    },
    ColorDataBaseEntry {
        name: b"grey24\0",
        code: 0xff3d3d3d,
    },
    ColorDataBaseEntry {
        name: b"grey25\0",
        code: 0xff404040,
    },
    ColorDataBaseEntry {
        name: b"grey26\0",
        code: 0xff424242,
    },
    ColorDataBaseEntry {
        name: b"grey27\0",
        code: 0xff454545,
    },
    ColorDataBaseEntry {
        name: b"grey28\0",
        code: 0xff474747,
    },
    ColorDataBaseEntry {
        name: b"grey29\0",
        code: 0xff4a4a4a,
    },
    ColorDataBaseEntry {
        name: b"grey30\0",
        code: 0xff4d4d4d,
    },
    ColorDataBaseEntry {
        name: b"grey31\0",
        code: 0xff4f4f4f,
    },
    ColorDataBaseEntry {
        name: b"grey32\0",
        code: 0xff525252,
    },
    ColorDataBaseEntry {
        name: b"grey33\0",
        code: 0xff545454,
    },
    ColorDataBaseEntry {
        name: b"grey34\0",
        code: 0xff575757,
    },
    ColorDataBaseEntry {
        name: b"grey35\0",
        code: 0xff595959,
    },
    ColorDataBaseEntry {
        name: b"grey36\0",
        code: 0xff5c5c5c,
    },
    ColorDataBaseEntry {
        name: b"grey37\0",
        code: 0xff5e5e5e,
    },
    ColorDataBaseEntry {
        name: b"grey38\0",
        code: 0xff616161,
    },
    ColorDataBaseEntry {
        name: b"grey39\0",
        code: 0xff636363,
    },
    ColorDataBaseEntry {
        name: b"grey40\0",
        code: 0xff666666,
    },
    ColorDataBaseEntry {
        name: b"grey41\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"grey42\0",
        code: 0xff6b6b6b,
    },
    ColorDataBaseEntry {
        name: b"grey43\0",
        code: 0xff6e6e6e,
    },
    ColorDataBaseEntry {
        name: b"grey44\0",
        code: 0xff707070,
    },
    ColorDataBaseEntry {
        name: b"grey45\0",
        code: 0xff737373,
    },
    ColorDataBaseEntry {
        name: b"grey46\0",
        code: 0xff757575,
    },
    ColorDataBaseEntry {
        name: b"grey47\0",
        code: 0xff787878,
    },
    ColorDataBaseEntry {
        name: b"grey48\0",
        code: 0xff7a7a7a,
    },
    ColorDataBaseEntry {
        name: b"grey49\0",
        code: 0xff7d7d7d,
    },
    ColorDataBaseEntry {
        name: b"grey50\0",
        code: 0xff7f7f7f,
    },
    ColorDataBaseEntry {
        name: b"grey51\0",
        code: 0xff828282,
    },
    ColorDataBaseEntry {
        name: b"grey52\0",
        code: 0xff858585,
    },
    ColorDataBaseEntry {
        name: b"grey53\0",
        code: 0xff878787,
    },
    ColorDataBaseEntry {
        name: b"grey54\0",
        code: 0xff8a8a8a,
    },
    ColorDataBaseEntry {
        name: b"grey55\0",
        code: 0xff8c8c8c,
    },
    ColorDataBaseEntry {
        name: b"grey56\0",
        code: 0xff8f8f8f,
    },
    ColorDataBaseEntry {
        name: b"grey57\0",
        code: 0xff919191,
    },
    ColorDataBaseEntry {
        name: b"grey58\0",
        code: 0xff949494,
    },
    ColorDataBaseEntry {
        name: b"grey59\0",
        code: 0xff969696,
    },
    ColorDataBaseEntry {
        name: b"grey60\0",
        code: 0xff999999,
    },
    ColorDataBaseEntry {
        name: b"grey61\0",
        code: 0xff9c9c9c,
    },
    ColorDataBaseEntry {
        name: b"grey62\0",
        code: 0xff9e9e9e,
    },
    ColorDataBaseEntry {
        name: b"grey63\0",
        code: 0xffa1a1a1,
    },
    ColorDataBaseEntry {
        name: b"grey64\0",
        code: 0xffa3a3a3,
    },
    ColorDataBaseEntry {
        name: b"grey65\0",
        code: 0xffa6a6a6,
    },
    ColorDataBaseEntry {
        name: b"grey66\0",
        code: 0xffa8a8a8,
    },
    ColorDataBaseEntry {
        name: b"grey67\0",
        code: 0xffababab,
    },
    ColorDataBaseEntry {
        name: b"grey68\0",
        code: 0xffadadad,
    },
    ColorDataBaseEntry {
        name: b"grey69\0",
        code: 0xffb0b0b0,
    },
    ColorDataBaseEntry {
        name: b"grey70\0",
        code: 0xffb3b3b3,
    },
    ColorDataBaseEntry {
        name: b"grey71\0",
        code: 0xffb5b5b5,
    },
    ColorDataBaseEntry {
        name: b"grey72\0",
        code: 0xffb8b8b8,
    },
    ColorDataBaseEntry {
        name: b"grey73\0",
        code: 0xffbababa,
    },
    ColorDataBaseEntry {
        name: b"grey74\0",
        code: 0xffbdbdbd,
    },
    ColorDataBaseEntry {
        name: b"grey75\0",
        code: 0xffbfbfbf,
    },
    ColorDataBaseEntry {
        name: b"grey76\0",
        code: 0xffc2c2c2,
    },
    ColorDataBaseEntry {
        name: b"grey77\0",
        code: 0xffc4c4c4,
    },
    ColorDataBaseEntry {
        name: b"grey78\0",
        code: 0xffc7c7c7,
    },
    ColorDataBaseEntry {
        name: b"grey79\0",
        code: 0xffc9c9c9,
    },
    ColorDataBaseEntry {
        name: b"grey80\0",
        code: 0xffcccccc,
    },
    ColorDataBaseEntry {
        name: b"grey81\0",
        code: 0xffcfcfcf,
    },
    ColorDataBaseEntry {
        name: b"grey82\0",
        code: 0xffd1d1d1,
    },
    ColorDataBaseEntry {
        name: b"grey83\0",
        code: 0xffd4d4d4,
    },
    ColorDataBaseEntry {
        name: b"grey84\0",
        code: 0xffd6d6d6,
    },
    ColorDataBaseEntry {
        name: b"grey85\0",
        code: 0xffd9d9d9,
    },
    ColorDataBaseEntry {
        name: b"grey86\0",
        code: 0xffdbdbdb,
    },
    ColorDataBaseEntry {
        name: b"grey87\0",
        code: 0xffdedede,
    },
    ColorDataBaseEntry {
        name: b"grey88\0",
        code: 0xffe0e0e0,
    },
    ColorDataBaseEntry {
        name: b"grey89\0",
        code: 0xffe3e3e3,
    },
    ColorDataBaseEntry {
        name: b"grey90\0",
        code: 0xffe5e5e5,
    },
    ColorDataBaseEntry {
        name: b"grey91\0",
        code: 0xffe8e8e8,
    },
    ColorDataBaseEntry {
        name: b"grey92\0",
        code: 0xffebebeb,
    },
    ColorDataBaseEntry {
        name: b"grey93\0",
        code: 0xffededed,
    },
    ColorDataBaseEntry {
        name: b"grey94\0",
        code: 0xfff0f0f0,
    },
    ColorDataBaseEntry {
        name: b"grey95\0",
        code: 0xfff2f2f2,
    },
    ColorDataBaseEntry {
        name: b"grey96\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"grey97\0",
        code: 0xfff7f7f7,
    },
    ColorDataBaseEntry {
        name: b"grey98\0",
        code: 0xfffafafa,
    },
    ColorDataBaseEntry {
        name: b"grey99\0",
        code: 0xfffcfcfc,
    },
    ColorDataBaseEntry {
        name: b"grey100\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"honeydew\0",
        code: 0xfff0fff0,
    },
    ColorDataBaseEntry {
        name: b"honeydew1\0",
        code: 0xfff0fff0,
    },
    ColorDataBaseEntry {
        name: b"honeydew2\0",
        code: 0xffe0eee0,
    },
    ColorDataBaseEntry {
        name: b"honeydew3\0",
        code: 0xffc1cdc1,
    },
    ColorDataBaseEntry {
        name: b"honeydew4\0",
        code: 0xff838b83,
    },
    ColorDataBaseEntry {
        name: b"hotpink\0",
        code: 0xffb469ff,
    },
    ColorDataBaseEntry {
        name: b"hotpink1\0",
        code: 0xffb46eff,
    },
    ColorDataBaseEntry {
        name: b"hotpink2\0",
        code: 0xffa76aee,
    },
    ColorDataBaseEntry {
        name: b"hotpink3\0",
        code: 0xff9060cd,
    },
    ColorDataBaseEntry {
        name: b"hotpink4\0",
        code: 0xff623a8b,
    },
    ColorDataBaseEntry {
        name: b"indianred\0",
        code: 0xff5c5ccd,
    },
    ColorDataBaseEntry {
        name: b"indianred1\0",
        code: 0xff6a6aff,
    },
    ColorDataBaseEntry {
        name: b"indianred2\0",
        code: 0xff6363ee,
    },
    ColorDataBaseEntry {
        name: b"indianred3\0",
        code: 0xff5555cd,
    },
    ColorDataBaseEntry {
        name: b"indianred4\0",
        code: 0xff3a3a8b,
    },
    ColorDataBaseEntry {
        name: b"ivory\0",
        code: 0xfff0ffff,
    },
    ColorDataBaseEntry {
        name: b"ivory1\0",
        code: 0xfff0ffff,
    },
    ColorDataBaseEntry {
        name: b"ivory2\0",
        code: 0xffe0eeee,
    },
    ColorDataBaseEntry {
        name: b"ivory3\0",
        code: 0xffc1cdcd,
    },
    ColorDataBaseEntry {
        name: b"ivory4\0",
        code: 0xff838b8b,
    },
    ColorDataBaseEntry {
        name: b"khaki\0",
        code: 0xff8ce6f0,
    },
    ColorDataBaseEntry {
        name: b"khaki1\0",
        code: 0xff8ff6ff,
    },
    ColorDataBaseEntry {
        name: b"khaki2\0",
        code: 0xff85e6ee,
    },
    ColorDataBaseEntry {
        name: b"khaki3\0",
        code: 0xff73c6cd,
    },
    ColorDataBaseEntry {
        name: b"khaki4\0",
        code: 0xff4e868b,
    },
    ColorDataBaseEntry {
        name: b"lavender\0",
        code: 0xfffae6e6,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush\0",
        code: 0xfff5f0ff,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush1\0",
        code: 0xfff5f0ff,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush2\0",
        code: 0xffe5e0ee,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush3\0",
        code: 0xffc5c1cd,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush4\0",
        code: 0xff86838b,
    },
    ColorDataBaseEntry {
        name: b"lawngreen\0",
        code: 0xff00fc7c,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon\0",
        code: 0xffcdfaff,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon1\0",
        code: 0xffcdfaff,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon2\0",
        code: 0xffbfe9ee,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon3\0",
        code: 0xffa5c9cd,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon4\0",
        code: 0xff70898b,
    },
    ColorDataBaseEntry {
        name: b"lightblue\0",
        code: 0xffe6d8ad,
    },
    ColorDataBaseEntry {
        name: b"lightblue1\0",
        code: 0xffffefbf,
    },
    ColorDataBaseEntry {
        name: b"lightblue2\0",
        code: 0xffeedfb2,
    },
    ColorDataBaseEntry {
        name: b"lightblue3\0",
        code: 0xffcdc09a,
    },
    ColorDataBaseEntry {
        name: b"lightblue4\0",
        code: 0xff8b8368,
    },
    ColorDataBaseEntry {
        name: b"lightcoral\0",
        code: 0xff8080f0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan\0",
        code: 0xffffffe0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan1\0",
        code: 0xffffffe0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan2\0",
        code: 0xffeeeed1,
    },
    ColorDataBaseEntry {
        name: b"lightcyan3\0",
        code: 0xffcdcdb4,
    },
    ColorDataBaseEntry {
        name: b"lightcyan4\0",
        code: 0xff8b8b7a,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod\0",
        code: 0xff82ddee,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod1\0",
        code: 0xff8becff,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod2\0",
        code: 0xff82dcee,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod3\0",
        code: 0xff70becd,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod4\0",
        code: 0xff4c818b,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrodyellow\0",
        code: 0xffd2fafa,
    },
    ColorDataBaseEntry {
        name: b"lightgray\0",
        code: 0xffd3d3d3,
    },
    ColorDataBaseEntry {
        name: b"lightgreen\0",
        code: 0xff90ee90,
    },
    ColorDataBaseEntry {
        name: b"lightgrey\0",
        code: 0xffd3d3d3,
    },
    ColorDataBaseEntry {
        name: b"lightpink\0",
        code: 0xffc1b6ff,
    },
    ColorDataBaseEntry {
        name: b"lightpink1\0",
        code: 0xffb9aeff,
    },
    ColorDataBaseEntry {
        name: b"lightpink2\0",
        code: 0xffada2ee,
    },
    ColorDataBaseEntry {
        name: b"lightpink3\0",
        code: 0xff958ccd,
    },
    ColorDataBaseEntry {
        name: b"lightpink4\0",
        code: 0xff655f8b,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon\0",
        code: 0xff7aa0ff,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon1\0",
        code: 0xff7aa0ff,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon2\0",
        code: 0xff7295ee,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon3\0",
        code: 0xff6281cd,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon4\0",
        code: 0xff42578b,
    },
    ColorDataBaseEntry {
        name: b"lightseagreen\0",
        code: 0xffaab220,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue\0",
        code: 0xffface87,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue1\0",
        code: 0xffffe2b0,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue2\0",
        code: 0xffeed3a4,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue3\0",
        code: 0xffcdb68d,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue4\0",
        code: 0xff8b7b60,
    },
    ColorDataBaseEntry {
        name: b"lightslateblue\0",
        code: 0xffff7084,
    },
    ColorDataBaseEntry {
        name: b"lightslategray\0",
        code: 0xff998877,
    },
    ColorDataBaseEntry {
        name: b"lightslategrey\0",
        code: 0xff998877,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue\0",
        code: 0xffdec4b0,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue1\0",
        code: 0xffffe1ca,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue2\0",
        code: 0xffeed2bc,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue3\0",
        code: 0xffcdb5a2,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue4\0",
        code: 0xff8b7b6e,
    },
    ColorDataBaseEntry {
        name: b"lightyellow\0",
        code: 0xffe0ffff,
    },
    ColorDataBaseEntry {
        name: b"lightyellow1\0",
        code: 0xffe0ffff,
    },
    ColorDataBaseEntry {
        name: b"lightyellow2\0",
        code: 0xffd1eeee,
    },
    ColorDataBaseEntry {
        name: b"lightyellow3\0",
        code: 0xffb4cdcd,
    },
    ColorDataBaseEntry {
        name: b"lightyellow4\0",
        code: 0xff7a8b8b,
    },
    ColorDataBaseEntry {
        name: b"limegreen\0",
        code: 0xff32cd32,
    },
    ColorDataBaseEntry {
        name: b"linen\0",
        code: 0xffe6f0fa,
    },
    ColorDataBaseEntry {
        name: b"magenta\0",
        code: 0xffff00ff,
    },
    ColorDataBaseEntry {
        name: b"magenta1\0",
        code: 0xffff00ff,
    },
    ColorDataBaseEntry {
        name: b"magenta2\0",
        code: 0xffee00ee,
    },
    ColorDataBaseEntry {
        name: b"magenta3\0",
        code: 0xffcd00cd,
    },
    ColorDataBaseEntry {
        name: b"magenta4\0",
        code: 0xff8b008b,
    },
    ColorDataBaseEntry {
        name: b"maroon\0",
        code: 0xff6030b0,
    },
    ColorDataBaseEntry {
        name: b"maroon1\0",
        code: 0xffb334ff,
    },
    ColorDataBaseEntry {
        name: b"maroon2\0",
        code: 0xffa730ee,
    },
    ColorDataBaseEntry {
        name: b"maroon3\0",
        code: 0xff9029cd,
    },
    ColorDataBaseEntry {
        name: b"maroon4\0",
        code: 0xff621c8b,
    },
    ColorDataBaseEntry {
        name: b"mediumaquamarine\0",
        code: 0xffaacd66,
    },
    ColorDataBaseEntry {
        name: b"mediumblue\0",
        code: 0xffcd0000,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid\0",
        code: 0xffd355ba,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid1\0",
        code: 0xffff66e0,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid2\0",
        code: 0xffee5fd1,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid3\0",
        code: 0xffcd52b4,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid4\0",
        code: 0xff8b377a,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple\0",
        code: 0xffdb7093,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple1\0",
        code: 0xffff82ab,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple2\0",
        code: 0xffee799f,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple3\0",
        code: 0xffcd6889,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple4\0",
        code: 0xff8b475d,
    },
    ColorDataBaseEntry {
        name: b"mediumseagreen\0",
        code: 0xff71b33c,
    },
    ColorDataBaseEntry {
        name: b"mediumslateblue\0",
        code: 0xffee687b,
    },
    ColorDataBaseEntry {
        name: b"mediumspringgreen\0",
        code: 0xff9afa00,
    },
    ColorDataBaseEntry {
        name: b"mediumturquoise\0",
        code: 0xffccd148,
    },
    ColorDataBaseEntry {
        name: b"mediumvioletred\0",
        code: 0xff8515c7,
    },
    ColorDataBaseEntry {
        name: b"midnightblue\0",
        code: 0xff701919,
    },
    ColorDataBaseEntry {
        name: b"mintcream\0",
        code: 0xfffafff5,
    },
    ColorDataBaseEntry {
        name: b"mistyrose\0",
        code: 0xffe1e4ff,
    },
    ColorDataBaseEntry {
        name: b"mistyrose1\0",
        code: 0xffe1e4ff,
    },
    ColorDataBaseEntry {
        name: b"mistyrose2\0",
        code: 0xffd2d5ee,
    },
    ColorDataBaseEntry {
        name: b"mistyrose3\0",
        code: 0xffb5b7cd,
    },
    ColorDataBaseEntry {
        name: b"mistyrose4\0",
        code: 0xff7b7d8b,
    },
    ColorDataBaseEntry {
        name: b"moccasin\0",
        code: 0xffb5e4ff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite\0",
        code: 0xffaddeff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite1\0",
        code: 0xffaddeff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite2\0",
        code: 0xffa1cfee,
    },
    ColorDataBaseEntry {
        name: b"navajowhite3\0",
        code: 0xff8bb3cd,
    },
    ColorDataBaseEntry {
        name: b"navajowhite4\0",
        code: 0xff5e798b,
    },
    ColorDataBaseEntry {
        name: b"navy\0",
        code: 0xff800000,
    },
    ColorDataBaseEntry {
        name: b"navyblue\0",
        code: 0xff800000,
    },
    ColorDataBaseEntry {
        name: b"oldlace\0",
        code: 0xffe6f5fd,
    },
    ColorDataBaseEntry {
        name: b"olivedrab\0",
        code: 0xff238e6b,
    },
    ColorDataBaseEntry {
        name: b"olivedrab1\0",
        code: 0xff3effc0,
    },
    ColorDataBaseEntry {
        name: b"olivedrab2\0",
        code: 0xff3aeeb3,
    },
    ColorDataBaseEntry {
        name: b"olivedrab3\0",
        code: 0xff32cd9a,
    },
    ColorDataBaseEntry {
        name: b"olivedrab4\0",
        code: 0xff228b69,
    },
    ColorDataBaseEntry {
        name: b"orange\0",
        code: 0xff00a5ff,
    },
    ColorDataBaseEntry {
        name: b"orange1\0",
        code: 0xff00a5ff,
    },
    ColorDataBaseEntry {
        name: b"orange2\0",
        code: 0xff009aee,
    },
    ColorDataBaseEntry {
        name: b"orange3\0",
        code: 0xff0085cd,
    },
    ColorDataBaseEntry {
        name: b"orange4\0",
        code: 0xff005a8b,
    },
    ColorDataBaseEntry {
        name: b"orangered\0",
        code: 0xff0045ff,
    },
    ColorDataBaseEntry {
        name: b"orangered1\0",
        code: 0xff0045ff,
    },
    ColorDataBaseEntry {
        name: b"orangered2\0",
        code: 0xff0040ee,
    },
    ColorDataBaseEntry {
        name: b"orangered3\0",
        code: 0xff0037cd,
    },
    ColorDataBaseEntry {
        name: b"orangered4\0",
        code: 0xff00258b,
    },
    ColorDataBaseEntry {
        name: b"orchid\0",
        code: 0xffd670da,
    },
    ColorDataBaseEntry {
        name: b"orchid1\0",
        code: 0xfffa83ff,
    },
    ColorDataBaseEntry {
        name: b"orchid2\0",
        code: 0xffe97aee,
    },
    ColorDataBaseEntry {
        name: b"orchid3\0",
        code: 0xffc969cd,
    },
    ColorDataBaseEntry {
        name: b"orchid4\0",
        code: 0xff89478b,
    },
    ColorDataBaseEntry {
        name: b"palegoldenrod\0",
        code: 0xffaae8ee,
    },
    ColorDataBaseEntry {
        name: b"palegreen\0",
        code: 0xff98fb98,
    },
    ColorDataBaseEntry {
        name: b"palegreen1\0",
        code: 0xff9aff9a,
    },
    ColorDataBaseEntry {
        name: b"palegreen2\0",
        code: 0xff90ee90,
    },
    ColorDataBaseEntry {
        name: b"palegreen3\0",
        code: 0xff7ccd7c,
    },
    ColorDataBaseEntry {
        name: b"palegreen4\0",
        code: 0xff548b54,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise\0",
        code: 0xffeeeeaf,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise1\0",
        code: 0xffffffbb,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise2\0",
        code: 0xffeeeeae,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise3\0",
        code: 0xffcdcd96,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise4\0",
        code: 0xff8b8b66,
    },
    ColorDataBaseEntry {
        name: b"palevioletred\0",
        code: 0xff9370db,
    },
    ColorDataBaseEntry {
        name: b"palevioletred1\0",
        code: 0xffab82ff,
    },
    ColorDataBaseEntry {
        name: b"palevioletred2\0",
        code: 0xff9f79ee,
    },
    ColorDataBaseEntry {
        name: b"palevioletred3\0",
        code: 0xff8968cd,
    },
    ColorDataBaseEntry {
        name: b"palevioletred4\0",
        code: 0xff5d478b,
    },
    ColorDataBaseEntry {
        name: b"papayawhip\0",
        code: 0xffd5efff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff\0",
        code: 0xffb9daff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff1\0",
        code: 0xffb9daff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff2\0",
        code: 0xffadcbee,
    },
    ColorDataBaseEntry {
        name: b"peachpuff3\0",
        code: 0xff95afcd,
    },
    ColorDataBaseEntry {
        name: b"peachpuff4\0",
        code: 0xff65778b,
    },
    ColorDataBaseEntry {
        name: b"peru\0",
        code: 0xff3f85cd,
    },
    ColorDataBaseEntry {
        name: b"pink\0",
        code: 0xffcbc0ff,
    },
    ColorDataBaseEntry {
        name: b"pink1\0",
        code: 0xffc5b5ff,
    },
    ColorDataBaseEntry {
        name: b"pink2\0",
        code: 0xffb8a9ee,
    },
    ColorDataBaseEntry {
        name: b"pink3\0",
        code: 0xff9e91cd,
    },
    ColorDataBaseEntry {
        name: b"pink4\0",
        code: 0xff6c638b,
    },
    ColorDataBaseEntry {
        name: b"plum\0",
        code: 0xffdda0dd,
    },
    ColorDataBaseEntry {
        name: b"plum1\0",
        code: 0xffffbbff,
    },
    ColorDataBaseEntry {
        name: b"plum2\0",
        code: 0xffeeaeee,
    },
    ColorDataBaseEntry {
        name: b"plum3\0",
        code: 0xffcd96cd,
    },
    ColorDataBaseEntry {
        name: b"plum4\0",
        code: 0xff8b668b,
    },
    ColorDataBaseEntry {
        name: b"powderblue\0",
        code: 0xffe6e0b0,
    },
    ColorDataBaseEntry {
        name: b"purple\0",
        code: 0xfff020a0,
    },
    ColorDataBaseEntry {
        name: b"purple1\0",
        code: 0xffff309b,
    },
    ColorDataBaseEntry {
        name: b"purple2\0",
        code: 0xffee2c91,
    },
    ColorDataBaseEntry {
        name: b"purple3\0",
        code: 0xffcd267d,
    },
    ColorDataBaseEntry {
        name: b"purple4\0",
        code: 0xff8b1a55,
    },
    ColorDataBaseEntry {
        name: b"red\0",
        code: 0xff0000ff,
    },
    ColorDataBaseEntry {
        name: b"red1\0",
        code: 0xff0000ff,
    },
    ColorDataBaseEntry {
        name: b"red2\0",
        code: 0xff0000ee,
    },
    ColorDataBaseEntry {
        name: b"red3\0",
        code: 0xff0000cd,
    },
    ColorDataBaseEntry {
        name: b"red4\0",
        code: 0xff00008b,
    },
    ColorDataBaseEntry {
        name: b"rosybrown\0",
        code: 0xff8f8fbc,
    },
    ColorDataBaseEntry {
        name: b"rosybrown1\0",
        code: 0xffc1c1ff,
    },
    ColorDataBaseEntry {
        name: b"rosybrown2\0",
        code: 0xffb4b4ee,
    },
    ColorDataBaseEntry {
        name: b"rosybrown3\0",
        code: 0xff9b9bcd,
    },
    ColorDataBaseEntry {
        name: b"rosybrown4\0",
        code: 0xff69698b,
    },
    ColorDataBaseEntry {
        name: b"royalblue\0",
        code: 0xffe16941,
    },
    ColorDataBaseEntry {
        name: b"royalblue1\0",
        code: 0xffff7648,
    },
    ColorDataBaseEntry {
        name: b"royalblue2\0",
        code: 0xffee6e43,
    },
    ColorDataBaseEntry {
        name: b"royalblue3\0",
        code: 0xffcd5f3a,
    },
    ColorDataBaseEntry {
        name: b"royalblue4\0",
        code: 0xff8b4027,
    },
    ColorDataBaseEntry {
        name: b"saddlebrown\0",
        code: 0xff13458b,
    },
    ColorDataBaseEntry {
        name: b"salmon\0",
        code: 0xff7280fa,
    },
    ColorDataBaseEntry {
        name: b"salmon1\0",
        code: 0xff698cff,
    },
    ColorDataBaseEntry {
        name: b"salmon2\0",
        code: 0xff6282ee,
    },
    ColorDataBaseEntry {
        name: b"salmon3\0",
        code: 0xff5470cd,
    },
    ColorDataBaseEntry {
        name: b"salmon4\0",
        code: 0xff394c8b,
    },
    ColorDataBaseEntry {
        name: b"sandybrown\0",
        code: 0xff60a4f4,
    },
    ColorDataBaseEntry {
        name: b"seagreen\0",
        code: 0xff578b2e,
    },
    ColorDataBaseEntry {
        name: b"seagreen1\0",
        code: 0xff9fff54,
    },
    ColorDataBaseEntry {
        name: b"seagreen2\0",
        code: 0xff94ee4e,
    },
    ColorDataBaseEntry {
        name: b"seagreen3\0",
        code: 0xff80cd43,
    },
    ColorDataBaseEntry {
        name: b"seagreen4\0",
        code: 0xff578b2e,
    },
    ColorDataBaseEntry {
        name: b"seashell\0",
        code: 0xffeef5ff,
    },
    ColorDataBaseEntry {
        name: b"seashell1\0",
        code: 0xffeef5ff,
    },
    ColorDataBaseEntry {
        name: b"seashell2\0",
        code: 0xffdee5ee,
    },
    ColorDataBaseEntry {
        name: b"seashell3\0",
        code: 0xffbfc5cd,
    },
    ColorDataBaseEntry {
        name: b"seashell4\0",
        code: 0xff82868b,
    },
    ColorDataBaseEntry {
        name: b"sienna\0",
        code: 0xff2d52a0,
    },
    ColorDataBaseEntry {
        name: b"sienna1\0",
        code: 0xff4782ff,
    },
    ColorDataBaseEntry {
        name: b"sienna2\0",
        code: 0xff4279ee,
    },
    ColorDataBaseEntry {
        name: b"sienna3\0",
        code: 0xff3968cd,
    },
    ColorDataBaseEntry {
        name: b"sienna4\0",
        code: 0xff26478b,
    },
    ColorDataBaseEntry {
        name: b"skyblue\0",
        code: 0xffebce87,
    },
    ColorDataBaseEntry {
        name: b"skyblue1\0",
        code: 0xffffce87,
    },
    ColorDataBaseEntry {
        name: b"skyblue2\0",
        code: 0xffeec07e,
    },
    ColorDataBaseEntry {
        name: b"skyblue3\0",
        code: 0xffcda66c,
    },
    ColorDataBaseEntry {
        name: b"skyblue4\0",
        code: 0xff8b704a,
    },
    ColorDataBaseEntry {
        name: b"slateblue\0",
        code: 0xffcd5a6a,
    },
    ColorDataBaseEntry {
        name: b"slateblue1\0",
        code: 0xffff6f83,
    },
    ColorDataBaseEntry {
        name: b"slateblue2\0",
        code: 0xffee677a,
    },
    ColorDataBaseEntry {
        name: b"slateblue3\0",
        code: 0xffcd5969,
    },
    ColorDataBaseEntry {
        name: b"slateblue4\0",
        code: 0xff8b3c47,
    },
    ColorDataBaseEntry {
        name: b"slategray\0",
        code: 0xff908070,
    },
    ColorDataBaseEntry {
        name: b"slategray1\0",
        code: 0xffffe2c6,
    },
    ColorDataBaseEntry {
        name: b"slategray2\0",
        code: 0xffeed3b9,
    },
    ColorDataBaseEntry {
        name: b"slategray3\0",
        code: 0xffcdb69f,
    },
    ColorDataBaseEntry {
        name: b"slategray4\0",
        code: 0xff8b7b6c,
    },
    ColorDataBaseEntry {
        name: b"slategrey\0",
        code: 0xff908070,
    },
    ColorDataBaseEntry {
        name: b"snow\0",
        code: 0xfffafaff,
    },
    ColorDataBaseEntry {
        name: b"snow1\0",
        code: 0xfffafaff,
    },
    ColorDataBaseEntry {
        name: b"snow2\0",
        code: 0xffe9e9ee,
    },
    ColorDataBaseEntry {
        name: b"snow3\0",
        code: 0xffc9c9cd,
    },
    ColorDataBaseEntry {
        name: b"snow4\0",
        code: 0xff89898b,
    },
    ColorDataBaseEntry {
        name: b"springgreen\0",
        code: 0xff7fff00,
    },
    ColorDataBaseEntry {
        name: b"springgreen1\0",
        code: 0xff7fff00,
    },
    ColorDataBaseEntry {
        name: b"springgreen2\0",
        code: 0xff76ee00,
    },
    ColorDataBaseEntry {
        name: b"springgreen3\0",
        code: 0xff66cd00,
    },
    ColorDataBaseEntry {
        name: b"springgreen4\0",
        code: 0xff458b00,
    },
    ColorDataBaseEntry {
        name: b"steelblue\0",
        code: 0xffb48246,
    },
    ColorDataBaseEntry {
        name: b"steelblue1\0",
        code: 0xffffb863,
    },
    ColorDataBaseEntry {
        name: b"steelblue2\0",
        code: 0xffeeac5c,
    },
    ColorDataBaseEntry {
        name: b"steelblue3\0",
        code: 0xffcd944f,
    },
    ColorDataBaseEntry {
        name: b"steelblue4\0",
        code: 0xff8b6436,
    },
    ColorDataBaseEntry {
        name: b"tan\0",
        code: 0xff8cb4d2,
    },
    ColorDataBaseEntry {
        name: b"tan1\0",
        code: 0xff4fa5ff,
    },
    ColorDataBaseEntry {
        name: b"tan2\0",
        code: 0xff499aee,
    },
    ColorDataBaseEntry {
        name: b"tan3\0",
        code: 0xff3f85cd,
    },
    ColorDataBaseEntry {
        name: b"tan4\0",
        code: 0xff2b5a8b,
    },
    ColorDataBaseEntry {
        name: b"thistle\0",
        code: 0xffd8bfd8,
    },
    ColorDataBaseEntry {
        name: b"thistle1\0",
        code: 0xffffe1ff,
    },
    ColorDataBaseEntry {
        name: b"thistle2\0",
        code: 0xffeed2ee,
    },
    ColorDataBaseEntry {
        name: b"thistle3\0",
        code: 0xffcdb5cd,
    },
    ColorDataBaseEntry {
        name: b"thistle4\0",
        code: 0xff8b7b8b,
    },
    ColorDataBaseEntry {
        name: b"tomato\0",
        code: 0xff4763ff,
    },
    ColorDataBaseEntry {
        name: b"tomato1\0",
        code: 0xff4763ff,
    },
    ColorDataBaseEntry {
        name: b"tomato2\0",
        code: 0xff425cee,
    },
    ColorDataBaseEntry {
        name: b"tomato3\0",
        code: 0xff394fcd,
    },
    ColorDataBaseEntry {
        name: b"tomato4\0",
        code: 0xff26368b,
    },
    ColorDataBaseEntry {
        name: b"turquoise\0",
        code: 0xffd0e040,
    },
    ColorDataBaseEntry {
        name: b"turquoise1\0",
        code: 0xfffff500,
    },
    ColorDataBaseEntry {
        name: b"turquoise2\0",
        code: 0xffeee500,
    },
    ColorDataBaseEntry {
        name: b"turquoise3\0",
        code: 0xffcdc500,
    },
    ColorDataBaseEntry {
        name: b"turquoise4\0",
        code: 0xff8b8600,
    },
    ColorDataBaseEntry {
        name: b"violet\0",
        code: 0xffee82ee,
    },
    ColorDataBaseEntry {
        name: b"violetred\0",
        code: 0xff9020d0,
    },
    ColorDataBaseEntry {
        name: b"violetred1\0",
        code: 0xff963eff,
    },
    ColorDataBaseEntry {
        name: b"violetred2\0",
        code: 0xff8c3aee,
    },
    ColorDataBaseEntry {
        name: b"violetred3\0",
        code: 0xff7832cd,
    },
    ColorDataBaseEntry {
        name: b"violetred4\0",
        code: 0xff52228b,
    },
    ColorDataBaseEntry {
        name: b"wheat\0",
        code: 0xffb3def5,
    },
    ColorDataBaseEntry {
        name: b"wheat1\0",
        code: 0xffbae7ff,
    },
    ColorDataBaseEntry {
        name: b"wheat2\0",
        code: 0xffaed8ee,
    },
    ColorDataBaseEntry {
        name: b"wheat3\0",
        code: 0xff96bacd,
    },
    ColorDataBaseEntry {
        name: b"wheat4\0",
        code: 0xff667e8b,
    },
    ColorDataBaseEntry {
        name: b"whitesmoke\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"yellow\0",
        code: 0xff00ffff,
    },
    ColorDataBaseEntry {
        name: b"yellow1\0",
        code: 0xff00ffff,
    },
    ColorDataBaseEntry {
        name: b"yellow2\0",
        code: 0xff00eeee,
    },
    ColorDataBaseEntry {
        name: b"yellow3\0",
        code: 0xff00cdcd,
    },
    ColorDataBaseEntry {
        name: b"yellow4\0",
        code: 0xff008b8b,
    },
    ColorDataBaseEntry {
        name: b"yellowgreen\0",
        code: 0xff32cd9a,
    },
];

// ---------------------------------------------------------------------------
// Color name/str lookup functions
// ---------------------------------------------------------------------------
