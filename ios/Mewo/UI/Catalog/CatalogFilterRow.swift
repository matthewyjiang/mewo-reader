import SwiftUI

private let allGenresTitle = "All genres"

/// Genre and sort menus at the top of the catalog list. Sort applies to browse only.
struct CatalogFilterRow: View {
    @Environment(\.mewo) private var mewo
    var filters: CatalogFilters
    var filter: CatalogFilter
    var searching: Bool
    var onChange: (CatalogFilter) -> Void

    private var firstSort: String? { filters.sorts.first?.id }
    private var sortId: String? { filter.sort ?? firstSort }

    private var genreLabel: String {
        filters.genres.first { $0.id == filter.genre }?.name ?? allGenresTitle
    }

    private var sortLabel: String {
        filters.sorts.first { $0.id == sortId }?.name ?? ""
    }

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            menu(genreLabel, identifier: "catalog-genre") {
                item(allGenresTitle, selected: filter.genre == nil) {
                    onChange(CatalogFilter(genre: nil, sort: filter.sort))
                }
                ForEach(filters.genres) { genre in
                    item(genre.name, selected: genre.id == filter.genre) {
                        onChange(CatalogFilter(genre: genre.id, sort: filter.sort))
                    }
                }
            }
            if !searching, !filters.sorts.isEmpty {
                menu(sortLabel, identifier: "catalog-sort") {
                    ForEach(filters.sorts) { sort in
                        item(sort.name, selected: sort.id == sortId) {
                            onChange(CatalogFilter(genre: filter.genre, sort: sort.id == firstSort ? nil : sort.id))
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private func menu<Items: View>(
        _ label: String,
        identifier: String,
        @ViewBuilder items: () -> Items
    ) -> some View {
        let content = items()
        return Menu {
            content
        } label: {
            HStack(alignment: .center, spacing: 4) {
                Text(label)
                    .font(MewoFont.bold(15))
                    .foregroundStyle(mewo.ink)
                    .lineLimit(1)
                Image(systemName: "chevron.down")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(mewo.ink)
                    .accessibilityHidden(true)
            }
            .padding(.leading, 16)
            .padding(.trailing, 12)
            .frame(height: 36)
            .background {
                Capsule().strokeBorder(mewo.line, lineWidth: 1)
            }
            .contentShape(Capsule())
        }
        .menuOrder(.fixed)
        .buttonStyle(.plain)
        .accessibilityIdentifier(identifier)
    }

    private func item(_ name: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            if selected {
                Label(name, systemImage: "checkmark")
            } else {
                Text(name)
            }
        }
    }
}
