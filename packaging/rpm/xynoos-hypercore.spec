Name:           xynoos-hypercore
Version:        0.1.0
Release:        1%{?dist}
Summary:        Universal Adaptive Linux Optimization Engine

License:        Apache-2.0 OR MIT
URL:            https://github.com/samuelindra123/xynoos-hypercore
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.82.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros
Requires:       systemd

%description
XYNOOS HyperCore is a universal, adaptive Linux optimization engine providing
continuous multi-resource pressure intelligence, dynamic memory and CPU coordination,
and reversible, safe system tuning.

%prep
%autosetup

%build
cargo build --frozen --release -p xynoos-cli --bin xynoos

%install
rm -rf %{buildroot}
install -D -p -m 0755 target/release/xynoos %{buildroot}%{_bindir}/xynoos
install -D -p -m 0644 systemd/xynoos-hypercore.service %{buildroot}%{_unitdir}/xynoos-hypercore.service
install -D -p -m 0644 debian/xynoos.1 %{buildroot}%{_mandir}/man1/xynoos.1
install -d -m 0755 %{buildroot}%{_sharedstatedir}/xynoos

%post
%systemd_post xynoos-hypercore.service

%preun
%systemd_preun xynoos-hypercore.service

%postun
%systemd_postun_with_restart xynoos-hypercore.service

%files
%license LICENSE-APACHE LICENSE-MIT
%doc README.md docs/
%{_bindir}/xynoos
%{_unitdir}/xynoos-hypercore.service
%{_mandir}/man1/xynoos.1*
%dir %attr(0755, root, root) %{_sharedstatedir}/xynoos

%changelog
* Wed Nov 26 2026 Samuel Indra Bastian <comdonate9@gmail.com> - 0.1.0-1
- Milestone O Universal Linux Compatibility packaging for RPM/Fedora/RHEL.
