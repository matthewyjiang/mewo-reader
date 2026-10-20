import Foundation
import Observation

/// Wait until the server URL stops changing before `GET /v1/instance`.
let instanceSignupDebounceNanos: UInt64 = 400_000_000

/// Loads `allowSignup` for the URL the user stopped typing.
/// A newer URL cancels the older request, and that response cannot publish.
@Observable
@MainActor
final class InstanceSignupLoader {
    private(set) var allowSignup: Bool?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private let debounceNanos: UInt64
    @ObservationIgnored private var fetch: @Sendable (String) async throws -> InstanceInfo
    @ObservationIgnored private let pause: @Sendable () async throws -> Void

    init(
        debounceNanos: UInt64 = instanceSignupDebounceNanos,
        fetch: @escaping @Sendable (String) async throws -> InstanceInfo = { _ in
            throw HostedException(message: "Set a server URL first.")
        },
        pause: (@Sendable () async throws -> Void)? = nil
    ) {
        self.debounceNanos = debounceNanos
        self.fetch = fetch
        self.pause = pause ?? { try await Task.sleep(nanoseconds: debounceNanos) }
    }

    /// Keeps this fetch, then loads `raw`. A newer call cancels the older request.
    func noteUrl(_ raw: String, fetch: @escaping @Sendable (String) async throws -> InstanceInfo) {
        self.fetch = fetch
        noteUrl(raw)
    }

    func noteUrl(_ raw: String) {
        task?.cancel()
        generation += 1
        let token = generation
        allowSignup = nil
        let url = normalizeBaseUrl(raw)
        guard !url.isEmpty else {
            task = nil
            return
        }
        let debounceNanos = debounceNanos
        let pause = pause
        let fetch = fetch
        task = Task { @MainActor in
            if debounceNanos > 0 {
                do {
                    try await pause()
                } catch {
                    return
                }
            }
            guard !Task.isCancelled, token == self.generation else { return }
            do {
                let info = try await fetch(url)
                guard !Task.isCancelled, token == self.generation else { return }
                self.allowSignup = info.allowSignup
            } catch is CancellationError {
                return
            } catch let error as URLError where error.code == .cancelled {
                return
            } catch {
                guard !Task.isCancelled, token == self.generation else { return }
                self.allowSignup = nil
            }
        }
    }

    func cancel() {
        task?.cancel()
        generation += 1
        task = nil
    }

    /// Tests await the request `noteUrl` just started.
    func settle() async {
        await task?.value
    }
}
