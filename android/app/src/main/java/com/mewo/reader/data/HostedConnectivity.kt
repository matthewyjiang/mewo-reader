package com.mewo.reader.data

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.Closeable

/**
 * One signal: the default network came back. The outbox drains on that edge.
 * There is no background sync while the process is gone.
 * Losing some other network does not count as offline.
 */
internal class HostedConnectivity(context: Context) : Closeable {
    private val manager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    private val _online = MutableStateFlow(false)
    val online: StateFlow<Boolean> = _online.asStateFlow()
    @Volatile
    private var current: Network? = null
    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            current = network
            _online.value = true
        }

        override fun onLost(network: Network) {
            if (network != current) return
            current = null
            _online.value = false
        }
    }

    init {
        current = manager.activeNetwork
        _online.value = current != null
        manager.registerDefaultNetworkCallback(callback)
    }

    override fun close() {
        manager.unregisterNetworkCallback(callback)
    }
}
