package com.soyeht.next.android

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import com.soyeht.next.android.theme.SoyehtTheme
import com.soyeht.next.android.ui.SoyehtApp

class MainActivity : ComponentActivity() {
    private val model: AppModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        offer(intent)
        setContent { SoyehtTheme { SoyehtApp(model) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        offer(intent)
    }

    private fun offer(intent: Intent?) {
        val link = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString ?: return
        model.offer(link)
        // The link is the pairing code: it must not stay in the intent the system keeps.
        intent.data = null
    }
}
