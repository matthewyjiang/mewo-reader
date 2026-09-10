package com.mewo.reader.ui.theme

import androidx.compose.ui.graphics.Color

// X lights-out. Night reading on a phone in bed.
val XBlack = Color(0xFF000000)
val XInk = Color(0xFFE7E9EA)
val XMute = Color(0xFF71767B)
val XLine = Color(0xFF2F3336)
val XBlue = Color(0xFF1D9BF0)
val XPink = Color(0xFFF91880)
val XGreen = Color(0xFF00BA7C)
val XHover = Color(0xFF16181C)

val XPaper = Color(0xFFFFFFFF)
val XInkLight = Color(0xFF0F1419)
val XMuteLight = Color(0xFF536471)
val XLineLight = Color(0xFFEFF3F4)
val XHoverLight = Color(0xFFF7F9F9)

// Twitter before the rebrand. Dim is the dark people mean.
val TwitterBlue = Color(0xFF1DA1F2)
val TwitterLike = Color(0xFFE0245E)
val TwitterGreen = Color(0xFF17BF63)
val TwitterInk = Color(0xFF14171A)
val TwitterMute = Color(0xFF657786)
val TwitterLine = Color(0xFFE1E8ED)
val TwitterHover = Color(0xFFF5F8FA)
val TwitterDim = Color(0xFF15202B)
val TwitterDimInk = Color(0xFFF7F9F9)
val TwitterDimMute = Color(0xFF8B98A5)
val TwitterDimLine = Color(0xFF38444D)
val TwitterDimHover = Color(0xFF1E2732)

data class MewoPalette(
    val ground: Color,
    val ink: Color,
    val mute: Color,
    val line: Color,
    val hover: Color,
    val accent: Color,
    val like: Color,
    val repost: Color,
    val onAccent: Color,
    val isDark: Boolean,
)

enum class DisplayTheme(
    val id: String,
    val label: String,
) {
    TwitterLight("twitter_light", "Twitter light"),
    TwitterDark("twitter_dim", "Twitter dim"),
    XLight("x_light", "X light"),
    XDark("x_lights_out", "Lights out"),
    ;

    val palette: MewoPalette
        get() = when (this) {
            TwitterLight -> MewoPalette(
                ground = XPaper,
                ink = TwitterInk,
                mute = TwitterMute,
                line = TwitterLine,
                hover = TwitterHover,
                accent = TwitterBlue,
                like = TwitterLike,
                repost = TwitterGreen,
                onAccent = XPaper,
                isDark = false,
            )
            TwitterDark -> MewoPalette(
                ground = TwitterDim,
                ink = TwitterDimInk,
                mute = TwitterDimMute,
                line = TwitterDimLine,
                hover = TwitterDimHover,
                accent = TwitterBlue,
                like = TwitterLike,
                repost = TwitterGreen,
                onAccent = XPaper,
                isDark = true,
            )
            XLight -> MewoPalette(
                ground = XPaper,
                ink = XInkLight,
                mute = XMuteLight,
                line = XLineLight,
                hover = XHoverLight,
                accent = XBlue,
                like = XPink,
                repost = XGreen,
                onAccent = XPaper,
                isDark = false,
            )
            XDark -> MewoPalette(
                ground = XBlack,
                ink = XInk,
                mute = XMute,
                line = XLine,
                hover = XHover,
                accent = XBlue,
                like = XPink,
                repost = XGreen,
                onAccent = XPaper,
                isDark = true,
            )
        }

    companion object {
        fun fromId(id: String?): DisplayTheme =
            entries.find { it.id == id } ?: XDark
    }
}

val AvatarPalette = listOf(
    Color(0xFF1D9BF0),
    Color(0xFFF91880),
    Color(0xFF00BA7C),
    Color(0xFFFF7A00),
    Color(0xFF7856FF),
    Color(0xFFFFAD1F),
)
