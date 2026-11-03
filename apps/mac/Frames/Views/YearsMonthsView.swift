import SwiftUI

/// One card per year, newest first. Clicking one shows its months.
struct YearsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 16)], spacing: 16) {
                ForEach(years, id: \.year) { year in
                    Button {
                        model.grouping = .months
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
struct MonthsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
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
    }
}

struct SummaryCard: View {
    let title: String
    let count: Int
    let keyAssetID: String
    let height: CGFloat

    var body: some View {
        Thumbnail(id: keyAssetID, pixels: 768)
            .frame(height: height)
            .frame(maxWidth: .infinity)
            .overlay(alignment: .bottomLeading) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.title2.bold())
                    Text("\(count) \(count == 1 ? "item" : "items")").font(.callout)
                }
                .foregroundStyle(.white)
                .shadow(radius: 4)
                .padding(14)
            }
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .contentShape(RoundedRectangle(cornerRadius: 12))
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
