package com.soyeht.next.android.theme

import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import com.soyeht.next.android.R

/**
 * The approved fonts (`fonts` of the token file): the chrome face of tabs and buttons, the UI face of text, and the
 * mono face of code and status words. The files are the Mac's, copied at build time (see `copyFonts`).
 */
@OptIn(ExperimentalTextApi::class)
object Faces {
    /** [Fonts.chrome]. */
    val chrome = FontFamily(
        Font(R.font.nunito_regular, FontWeight.Normal),
        Font(R.font.nunito_semibold, FontWeight.SemiBold),
        Font(R.font.nunito_bold, FontWeight.Bold),
    )

    /** [Fonts.ui]: one variable file, set to each weight. */
    val ui = FontFamily(
        listOf(FontWeight.Normal, FontWeight.Medium, FontWeight.SemiBold, FontWeight.Bold, FontWeight.ExtraBold)
            .map { weight ->
                Font(R.font.inter, weight, variationSettings = FontVariation.Settings(FontVariation.weight(weight.weight)))
            },
    )

    /** [Fonts.mono]. */
    val mono = FontFamily(
        Font(R.font.jetbrainsmono_regular, FontWeight.Normal),
        Font(R.font.jetbrainsmono_medium, FontWeight.Medium),
        Font(R.font.jetbrainsmono_bold, FontWeight.Bold),
    )
}
