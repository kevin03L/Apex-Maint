package fr.apex.maint.data

/** État du réseau par défaut, indépendant du cache parfois retardé de ConnectivityManager. */
class NetworkState<N>(private val publish: (Boolean) -> Unit) {
    private var current: N? = null
    @Synchronized fun refresh(network: N?) {
        current = network
        publish(network != null)
    }
    @Synchronized fun available(network: N) { current = network; publish(true) }
    @Synchronized fun lost(network: N) {
        if (network == current) { current = null; publish(false) }
    }
}
