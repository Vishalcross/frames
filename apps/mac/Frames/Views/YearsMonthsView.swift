import SwiftUI

/// One card per year, newest first. Clicking one shows its months.
struct YearsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 16)], spacing: 16) {
                ForEach(years, id: \.year) { year in
                    Button {
                        model.showYear(year.year)
                    } label: {
                        SummaryCard(title: String(year.year), count: year.count, keyAssetID: year.keyAssetID, height: 220)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(16)
        }
    }

    /// The library's months added up by year; each year's cover is its newest asset.
    private var years: [(year: Int, count: Int, keyAssetID: String)] {
        var result: [(year: Int, count: Int, keyAssetID: String)] = []
        for section in model.timeline.sections {
            if let last = result.last, last.year == section.year {
                result[result.count - 1].count += section.count
            } else {
                result.append((section.year, section.count, model.timeline.id(at: section.start)))
            }
        }
        return result
    }
}

/// One card per month, newest first. Clicking one shows it in All Photos.
/// Opened from a year, it starts at that year's months.
struct MonthsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 260), spacing: 14)], spacing: 14) {
                    ForEach(model.timeline.sections, id: \.start) { section in
                        Button {
                            model.showMonth(year: section.year, month: section.month)
                        } label: {
                            SummaryCard(title: section.title, count: section.count,
                                        keyAssetID: model.timeline.id(at: section.start), height: 180)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(16)
            }
            .onAppear {
                guard let year = model.monthsYear else { return }
                model.monthsYear = nil
                if let section = model.timeline.sections.first(where: { $0.year == year }) {
                    proxy.scrollTo(section.start, anchor: .top)
                }
            }
        }
    }
}

struct SummaryCard: View {
    let title: String
    let count: Int
    let keyAssetID: String
    let height: CGFloat

    @State private var hovering = false

    var body: some View {
        Thumbnail(id: keyAssetID, pixels: 768)
            .frame(height: height)
            .frame(maxWidth: .infinity)
            // A shade under the words, so they read on bright photos too.
            .overlay {
                LinearGradient(stops: [.init(color: .clear, location: 0.45), .init(color: .black.opacity(0.6), location: 1)],
                               startPoint: .top, endPoint: .bottom)
            }
            .overlay(alignment: .bottomLeading) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.title2.bold())
                    Text("\(count.formatted()) \(count == 1 ? "item" : "items")").font(.callout).opacity(0.85)
                }
                .foregroundStyle(.white)
                .shadow(color: .black.opacity(0.35), radius: 3, y: 1)
                .padding(14)
            }
            .clipShape(.rect(cornerRadius: 12, style: .continuous))
            .contentShape(.rect(cornerRadius: 12, style: .continuous))
            .scaleEffect(hovering ? 1.015 : 1)
            .animation(.easeOut(duration: 0.15), value: hovering)
            .onHover { hovering = $0 }
    }
}

/// An asset's thumbnail, decoded for `pixels` on the longer side.
struct Thumbnail: View {
    @Environment(AppModel.self) private var model
    let id: String
    let pixels: Int
    @State private var image: CGImage?

    var body: some View {
        Rectangle()
            .fill(.quaternary)
            .overlay {
                if let image {
                    Image(decorative: image, scale: 1).resizable().scaledToFill()
                }
            }
            .clipped()
            .onAppear {
                model.pipeline?.request(id, pixels: pixels) { image = $0 }
            }
    }
}
