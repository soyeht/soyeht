import SoyehtClient
import SoyehtProtocol
import SwiftUI

extension View {
  /// The Done button that closes a writing sheet.
  func phoneWritingDoneToolbar(
    minimumTarget: Double, dismiss: DismissAction
  ) -> some View {
    toolbar {
      ToolbarItem(placement: .confirmationAction) {
        Button {
          dismiss()
        } label: {
          PhoneWritingActionLabel(title: "Done", minimumTarget: minimumTarget)
        }
      }
    }
  }

  func phoneWritingSheet(_ kind: PhoneWritingSheetKind) -> some View {
    modifier(
      PhoneWritingSheetSurface(plan: PhoneWritingMetrics.sheetPolicy.plan(for: kind)))
  }

  func phoneWritingSheetScroll() -> some View {
    scrollContentBackground(.hidden)
      .background(PhonePalette.canvas)
  }

  func phoneWritingSheetRow() -> some View {
    foregroundStyle(PhonePalette.ink)
      .listRowBackground(PhonePalette.face)
  }

  func phoneWritingSheetNavigation(_ title: String) -> some View {
    navigationTitle(title)
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .principal) {
          Text(title)
            .font(.headline)
            .foregroundStyle(PhonePalette.ink)
        }
      }
  }
}

struct PhoneWritingSheetSurface: ViewModifier {
  let plan: PhoneWritingSheetPlan

  func body(content: Content) -> some View {
    content
      .foregroundStyle(PhonePalette.ink)
      .tint(PhonePalette.accent)
      .background(PhonePalette.canvas.ignoresSafeArea())
      .toolbarBackground(PhonePalette.face, for: .navigationBar)
      .toolbarBackground(.visible, for: .navigationBar)
      .presentationDetents(Set(plan.detents.map(\.presentationDetent)))
  }
}

extension PhoneWritingSheetDetent {
  var presentationDetent: PresentationDetent {
    switch self {
    case .medium: .medium
    case .large: .large
    }
  }
}
