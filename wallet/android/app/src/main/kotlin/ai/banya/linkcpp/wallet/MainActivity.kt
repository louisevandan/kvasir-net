package ai.banya.linkcpp.wallet

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController

// FragmentActivity (not ComponentActivity) so BiometricPrompt can host on this activity.
class MainActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            LinkcppTheme {
                val vm: WalletViewModel = viewModel()
                LaunchedEffect(Unit) { vm.restoreIfNeeded() }
                val nav = rememberNavController()
                CompositionLocalProvider(LocalStrings provides remember(vm.language) { Strings(vm.language) }) {
                    NavHost(nav, startDestination = if (vm.hasWallet) "home" else "welcome") {
                        composable("welcome") { WelcomeScreen(nav) }
                        composable("create") { CreateScreen(vm, nav) }
                        composable("import") { ImportScreen(vm, nav) }
                        composable("home") { HomeScreen(vm, nav) }
                        composable("history") { TransactionHistoryScreen(vm, nav) }
                        composable("receive") { ReceiveScreen(vm, nav) }
                        composable("send") { SendScreen(vm, nav) }
                        composable("staking") { StakingScreen(vm, nav) }
                        composable("nodemonitor") { NodeMonitorScreen(vm, nav) }
                        composable("deviceconnect") { DeviceConnectScreen(vm, nav) }
                        composable("nodesettings") { NodeSettingsScreen(vm, nav) }
                        composable("guide") { StakingGuideScreen(nav) }
                        composable("inference") { InferenceScreen(vm, nav) }
                        composable("models") { ModelsScreen(vm, nav) }
                        composable("export") { ExportPhraseScreen(vm, nav) }
                    }
                }
            }
        }
    }
}
