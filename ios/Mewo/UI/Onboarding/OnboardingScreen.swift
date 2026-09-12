import SwiftUI

struct OnboardingScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @State private var name = ""
    @State private var handle = ""
    @State private var submitError: String?

    var body: some View {
        let parsed = LocalIdentity.parse(name: name, handle: handle)
        VStack(spacing: 0) {
            MewoAppBar(leading: { Color.clear.frame(width: 1) }, center: { BrandMark() })
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Text("This is you")
                        .font(MewoFont.bold(22))
                        .foregroundStyle(mewo.ink)
                    Spacer().frame(height: 8)
                    Text("The drawer and your notes use this.")
                        .font(MewoFont.regular(17))
                        .foregroundStyle(mewo.mute)
                    Spacer().frame(height: 24)
                    ProfilePreview(name: name, handle: handle)
                    Spacer().frame(height: 20)
                    ProfileForm(name: $name, handle: $handle)
                    if let submitError {
                        Spacer().frame(height: 8)
                        Text(submitError).font(MewoFont.regular(15)).foregroundStyle(mewo.like)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 24)
                .padding(.bottom, 16)
            }
            Hairline()
            HStack {
                Spacer()
                PillButton(title: "Next", enabled: parsed.isSuccess) {
                    if let profile = parsed.profile {
                        do {
                            try app.localProfile.save(name: profile.name, handle: profile.handle)
                        } catch {
                            submitError = error.localizedDescription
                        }
                    } else {
                        submitError = parsed.error ?? "Could not save that."
                    }
                }
            }
            .frame(height: 56)
            .padding(.horizontal, 16)
        }
        .background(mewo.ground)
        .onChange(of: name) { _, _ in submitError = nil }
        .onChange(of: handle) { _, _ in submitError = nil }
    }
}
