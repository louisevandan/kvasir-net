import SwiftUI
import UIKit

// MARK: - Brand palette (derived from the app icon: pastel pink + sky blue)

extension Color {
    init(hex: UInt, alpha: Double = 1) {
        self.init(.sRGB,
                  red: Double((hex >> 16) & 0xff) / 255,
                  green: Double((hex >> 8) & 0xff) / 255,
                  blue: Double(hex & 0xff) / 255,
                  opacity: alpha)
    }
    /// A color that adapts to light/dark mode.
    static func dynamic(light: UInt, dark: UInt) -> Color {
        Color(UIColor { trait in
            UIColor(trait.userInterfaceStyle == .dark ? Color(hex: dark) : Color(hex: light))
        })
    }
}

enum Brand {
    static let pink       = Color.dynamic(light: 0xEC6FA6, dark: 0xF490BA)  // primary accent
    static let blue       = Color.dynamic(light: 0x6FA8E6, dark: 0x9AC3F5)  // secondary accent

    static let bgTop      = Color.dynamic(light: 0xFDEFF5, dark: 0x1B1522)
    static let bgBottom   = Color.dynamic(light: 0xE9F2FE, dark: 0x121722)

    static let card       = Color.dynamic(light: 0xFFFFFF, dark: 0x241E2E)
    static let stroke     = Color.dynamic(light: 0xF1E2EA, dark: 0x352C40)

    static let textPrimary   = Color.dynamic(light: 0x2C2533, dark: 0xF3ECF6)
    static let textSecondary = Color.dynamic(light: 0x938A9C, dark: 0xACA1B8)
    static let warn          = Color.dynamic(light: 0xC77E1E, dark: 0xE0952B)  // caution / gating notice

    static var gradient: LinearGradient {
        LinearGradient(colors: [pink, blue], startPoint: .topLeading, endPoint: .bottomTrailing)
    }
    static var softGradient: LinearGradient {
        LinearGradient(colors: [pink.opacity(0.18), blue.opacity(0.18)],
                       startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

// MARK: - Reusable pieces

/// Full-screen brand gradient background.
struct BrandBackground: View {
    var body: some View {
        LinearGradient(colors: [Brand.bgTop, Brand.bgBottom], startPoint: .top, endPoint: .bottom)
            .ignoresSafeArea()
    }
}

/// Rounded elevated card surface.
struct CardModifier: ViewModifier {
    var padding: CGFloat = 20
    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(Brand.card, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(Brand.stroke, lineWidth: 1)
            )
            .shadow(color: Brand.pink.opacity(0.10), radius: 18, x: 0, y: 10)
    }
}

extension View {
    func brandCard(padding: CGFloat = 20) -> some View { modifier(CardModifier(padding: padding)) }
}

// MARK: - Button styles

struct PrimaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(.headline, design: .rounded).weight(.semibold))
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 16)
            .background(Brand.gradient, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .shadow(color: Brand.pink.opacity(0.35), radius: 12, y: 6)
            .opacity(configuration.isPressed ? 0.85 : 1)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

struct SecondaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(.headline, design: .rounded).weight(.medium))
            .foregroundStyle(Brand.pink)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 16)
            .background(Brand.pink.opacity(0.12), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .opacity(configuration.isPressed ? 0.8 : 1)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

extension ButtonStyle where Self == PrimaryButtonStyle {
    static var brandPrimary: PrimaryButtonStyle { .init() }
}
extension ButtonStyle where Self == SecondaryButtonStyle {
    static var brandSecondary: SecondaryButtonStyle { .init() }
}

// MARK: - Rounded title helper

extension Text {
    func brandTitle() -> some View {
        self.font(.system(.largeTitle, design: .rounded).weight(.bold))
            .foregroundStyle(Brand.textPrimary)
    }
}
