import Foundation
import Network

/// One callback, on the edge from unsatisfied to satisfied. The first path is
/// the starting point, so launch does not look like a regain. There is no
/// background sync while the process is suspended.
@MainActor
final class HostedConnectivity {
    nonisolated private let monitor = NWPathMonitor()
    private var satisfied = false
    private var seeded = false
    private let onRegain: @MainActor () -> Void

    init(onRegain: @escaping @MainActor () -> Void) {
        self.onRegain = onRegain
        monitor.pathUpdateHandler = { [weak self] path in
            let ok = path.status == .satisfied
            Task { @MainActor in
                self?.note(ok)
            }
        }
        monitor.start(queue: DispatchQueue(label: "mewo.hosted.connectivity"))
    }

    func cancel() {
        monitor.cancel()
    }

    deinit {
        monitor.cancel()
    }

    private func note(_ ok: Bool) {
        if !seeded {
            seeded = true
            satisfied = ok
            return
        }
        let was = satisfied
        satisfied = ok
        if ok && !was {
            onRegain()
        }
    }
}
