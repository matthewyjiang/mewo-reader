package com.mewo.reader.ui.onboarding

import android.os.Build
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.LocalIdentity
import com.mewo.reader.ui.LocalProfile
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileForm
import com.mewo.reader.ui.components.ProfilePreview
import kotlinx.coroutines.CancellationException

@Composable
fun OnboardingScreen() {
    val profile = LocalProfile.current
    val activity = LocalActivity.current
    var name by rememberSaveable { mutableStateOf("") }
    var handle by rememberSaveable { mutableStateOf("") }
    var submitError by rememberSaveable { mutableStateOf<String?>(null) }
    val parsed = LocalIdentity.parse(name, handle)
    val canContinue = parsed.isSuccess

    PredictiveBackHandler(enabled = activity != null) { progress ->
        try {
            progress.collect { }
        } catch (e: CancellationException) {
            throw e
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity?.moveTaskToBack(true)
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .imePadding(),
    ) {
        MewoAppBar(center = { BrandMark() })
        Column(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp)
                .padding(top = 24.dp, bottom = 16.dp),
        ) {
            Text(
                text = "This is you",
                style = MaterialTheme.typography.headlineLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                text = "The drawer and your notes use this.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(24.dp))
            ProfilePreview(name = name, handle = handle)
            Spacer(Modifier.height(20.dp))
            ProfileForm(
                name = name,
                handle = handle,
                onNameChange = {
                    name = it
                    submitError = null
                },
                onHandleChange = {
                    handle = it
                    submitError = null
                },
            )
            submitError?.let { message ->
                Spacer(Modifier.height(8.dp))
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .height(56.dp)
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Spacer(Modifier.weight(1f))
            NextPill(
                enabled = canContinue,
                onClick = {
                    parsed
                        .onSuccess { profile.save(it.name, it.handle) }
                        .onFailure { err ->
                            submitError = err.message ?: "Could not save that."
                        }
                },
            )
        }
    }
}

@Composable
private fun NextPill(
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val accent = MaterialTheme.colorScheme.primary
    Text(
        text = "Next",
        color = MaterialTheme.colorScheme.onPrimary,
        style = MaterialTheme.typography.labelLarge,
        fontWeight = FontWeight.Bold,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (enabled) accent else accent.copy(alpha = 0.35f))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .semantics { contentDescription = "Next" },
    )
}
