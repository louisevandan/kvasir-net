import SwiftUI

/// Manage the models this phone has downloaded: see each GGUF and its size, and
/// delete ones no longer needed. Downloaded models also appear in AI 추론's model
/// picker for on-device inference.
struct ModelsManagerView: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject private var loc = Localizer.shared
    @StateObject private var store = ModelStore.shared
    @State private var pendingDelete: LocalModel?

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                if store.models.isEmpty {
                    emptyState
                } else {
                    ScrollView {
                        VStack(spacing: 12) {
                            headerCard
                            ForEach(store.models) { model in modelRow(model) }
                        }
                        .padding(20)
                    }
                }
            }
            .navigationTitle(loc.t("models.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } }
            }
            .task { store.refresh() }
            .confirmationDialog(loc.t("models.deleteConfirm"),
                                isPresented: Binding(get: { pendingDelete != nil },
                                                     set: { if !$0 { pendingDelete = nil } }),
                                titleVisibility: .visible) {
                Button(loc.t("models.delete"), role: .destructive) {
                    if let m = pendingDelete { store.delete(m) }
                    pendingDelete = nil
                }
                Button(loc.t("common.cancel"), role: .cancel) { pendingDelete = nil }
            }
        }
        .tint(Brand.pink)
    }

    private var headerCard: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(loc.t("models.storedTitle")).font(.system(.headline, design: .rounded))
                .foregroundStyle(Brand.textPrimary)
            Text(loc.t("models.storedDesc")).font(.caption).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func modelRow(_ model: LocalModel) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "cpu").font(.title3).foregroundStyle(Brand.blue)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.displayName).font(.system(.subheadline, design: .rounded).weight(.semibold))
                    .foregroundStyle(Brand.textPrimary).lineLimit(1)
                Text(model.sizeLabel).font(.caption2).foregroundStyle(Brand.textSecondary)
            }
            Spacer()
            Button(role: .destructive) { pendingDelete = model } label: {
                Image(systemName: "trash").foregroundStyle(.red)
            }
            .buttonStyle(.plain)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "square.stack.3d.up.slash")
                .font(.system(size: 44)).foregroundStyle(Brand.textSecondary)
            Text(loc.t("models.emptyTitle")).font(.system(.headline, design: .rounded))
                .foregroundStyle(Brand.textPrimary)
            Text(loc.t("models.emptyHint")).font(.callout).foregroundStyle(Brand.textSecondary)
                .multilineTextAlignment(.center).padding(.horizontal, 40)
        }
    }
}
